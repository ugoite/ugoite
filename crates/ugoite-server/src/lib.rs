//! Thin HTTP and MCP adapters over `ugoite-core`.

use anyhow::Context as _;
use axum::{
    extract::{
        DefaultBodyLimit, Extension, Form, Multipart, OriginalUri, Path, Query, Request, State,
    },
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use ugoite_core::error::{AppError, ErrorCode, ErrorKind};
use ugoite_domain::id::{validate_identifier, IdentifierKind};
use ugoite_domain::identity::{
    AccessPolicy, AccountStatus, Action, Actor, AgentMode, AssuranceLevel, AuthenticatedSubject,
    BindingMethod, CredentialConstraints, HumanAccount, NodeRole, PrincipalKind, PrincipalState,
    RequestAuthenticationMethod, RequestIdentity, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::{
    audit::{self, AuditListOptions},
    authorization::{AuthorizationState, Authorizer, ResourceKind},
    form, saved_sql,
    service::{SpacePermission, UgoiteService, MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS},
    space,
};
use ugoite_identity::{
    node_identity::{
        AccountInvitation, NodeAuditInput, NodeIdentityService, OwnerRecoveryContext,
        RecoveryBindingSnapshot, TotpEnrollmentFinishError,
    },
    oauth::{self, AccessTokenClaims, Confirmation},
};
use uuid::{Uuid, Version};
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

pub const OPENAPI_JSON: &str = include_str!("openapi.json");
const OAUTH_RESOURCE_DOCUMENTATION_URL: &str =
    "https://ugoite.github.io/ugoite/docs/guide/operate/auth/auth-overview/";

#[derive(Clone)]
pub struct AppState {
    service: UgoiteService,
    identity: NodeIdentityService,
}

impl AppState {
    pub fn new(root_uri: impl Into<String>) -> anyhow::Result<Self> {
        let root_uri = root_uri.into();
        let service = UgoiteService::new(root_uri)?;
        let public_origin = env::var("UGOITE_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:8000".to_string());
        let rp_id = env::var("UGOITE_WEBAUTHN_RP_ID").unwrap_or_else(|_| {
            url::Url::parse(&public_origin)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| "localhost".to_string())
        });
        let control_operator = match env::var("UGOITE_NODE_CONTROL_URI") {
            Ok(uri) => ugoite_storage::operator_from_uri(&uri)
                .context("configure UGOITE_NODE_CONTROL_URI")?,
            Err(_) => service.operator().clone(),
        };
        Ok(Self {
            identity: NodeIdentityService::new(control_operator, rp_id, public_origin)?,
            service,
        })
    }

    #[doc(hidden)]
    pub fn new_for_tests(root_uri: impl Into<String>) -> anyhow::Result<Self> {
        let service = UgoiteService::new(root_uri.into())?;
        Ok(Self {
            identity: NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?,
            service,
        })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(env::var("UGOITE_ROOT").unwrap_or_else(|_| "./data".to_string()))
    }

    fn workspace(&self, space_id: &str) -> String {
        self.service.workspace_path(space_id)
    }

    pub async fn initialize_node(&self) -> anyhow::Result<()> {
        if let Some(bootstrap) = self.identity.bootstrap_if_needed().await? {
            println!(
                "Ugoite setup URL (expires {}): {}",
                bootstrap.expires_at, bootstrap.setup_url
            );
        }
        for space_id in self.service.list_space_ids().await? {
            reconcile_recovery_fences(self, &space_id).await?;
            reconcile_recovery_audit_outbox(self, &space_id).await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    detail: Value,
}

impl ApiError {
    fn new(status: StatusCode, detail: impl Into<Value>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    fn from_core(error: anyhow::Error) -> Self {
        if let Some(app_error) = error.downcast_ref::<AppError>() {
            let status = match app_error.kind() {
                ErrorKind::InvalidInput => StatusCode::UNPROCESSABLE_ENTITY,
                ErrorKind::Forbidden => StatusCode::FORBIDDEN,
                ErrorKind::NotFound => StatusCode::NOT_FOUND,
                ErrorKind::Conflict => StatusCode::CONFLICT,
                ErrorKind::Expired => StatusCode::GONE,
                ErrorKind::Unimplemented => StatusCode::NOT_IMPLEMENTED,
                ErrorKind::DependencyUnavailable => StatusCode::BAD_GATEWAY,
                ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let mut detail = json!({
                "code": app_error.code_str(),
                "message": app_error.message(),
            });
            if let Some(extra) = app_error.detail() {
                detail["detail"] = extra.clone();
            }
            return Self { status, detail };
        }
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: json!({
                "code": "INTERNAL_ERROR",
                "message": "Internal server error",
            }),
        }
    }

    fn invalid_identifier(kind: IdentifierKind, error: ugoite_domain::id::IdentifierError) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            detail: json!({
                "code": "INVALID_IDENTIFIER",
                "message": format!("Invalid {}: {}", kind.as_str(), error.reason()),
            }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.detail.is_object() {
            (self.status, Json(self.detail)).into_response()
        } else {
            (self.status, Json(json!({ "detail": self.detail }))).into_response()
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone, Debug)]
struct RequestIdentityContext {
    request_identity: RequestIdentity,
    account_id: Uuid,
    display_name: String,
    node_admin: bool,
    token_principal_id: Option<Uuid>,
    token_actor_principal_id: Option<Uuid>,
    token_space_uid: Option<Uuid>,
    token_actions: Option<BTreeSet<String>>,
    recent_passkey: bool,
    credential_generation: u64,
}

fn protected_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/oauth/authorize",
            get(oauth_authorize).post(oauth_authorize_approve),
        )
        .route("/oauth/device/approve", post(oauth_device_approve))
        .route("/oauth/device/pending", get(oauth_device_pending))
        .route("/auth/devices", get(list_device_credentials))
        .route(
            "/auth/devices/{credential_id}",
            delete(revoke_device_credential),
        )
        .route("/auth/accounts", get(list_accounts))
        .route("/auth/audit", get(list_node_audit_events))
        .route(
            "/auth/accounts/{account_id}/status",
            post(set_account_status),
        )
        .route("/auth/oidc/providers", post(configure_oidc_provider))
        .route("/auth/passkeys", get(list_passkeys))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{session_id}", delete(revoke_session_by_id))
        .route("/auth/passkeys/start", post(start_add_passkey))
        .route("/auth/passkeys/finish", post(finish_add_passkey))
        .route("/auth/passkeys/{credential_id}", delete(revoke_passkey))
        .route("/auth/recovery/totp/start", post(start_totp_enrollment))
        .route("/auth/recovery/totp/finish", post(finish_totp_enrollment))
        .route(
            "/spaces/{space_id}/admin/recovery/force-reset",
            post(owner_force_reset),
        )
        .route(
            "/spaces/{space_id}/admin/recovery/backup-codes",
            post(owner_rotate_backup_codes),
        )
        .route(
            "/auth/invitations/accept",
            post(auth_invitation_accept_existing),
        )
        .route("/auth/oidc/{provider_id}/link", get(oidc_link_start))
        .route(
            "/spaces/{space_id}/agents",
            get(list_agents).post(create_agent),
        )
        .route("/spaces/{space_id}/agents/{agent_id}", delete(revoke_agent))
        .route(
            "/spaces/{space_id}/agents/{agent_id}/delegated-token",
            post(issue_delegated_agent_token),
        )
        .route(
            "/spaces/{space_id}/policies/{kind}/{resource_id}",
            get(get_access_policy).put(put_access_policy),
        )
        .route("/spaces", get(list_spaces).post(create_space))
        .route("/spaces/{space_id}", get(get_space).patch(patch_space))
        .route("/spaces/{space_id}/health", get(space_health))
        .route("/spaces/{space_id}/checkpoints", post(create_checkpoint))
        .route("/spaces/{space_id}/checkpoints/diff", get(checkpoint_diff))
        .route("/spaces/{space_id}/audit", get(list_audit_events))
        .route("/spaces/{space_id}/test-connection", post(test_connection))
        .route(
            "/preferences/me",
            get(get_preferences).patch(patch_preferences),
        )
        .route("/spaces/{space_id}/members", get(list_members))
        .route(
            "/spaces/{space_id}/members/invitations",
            post(invite_member),
        )
        .route(
            "/spaces/{space_id}/members/{principal_id}/role",
            post(update_member_role),
        )
        .route(
            "/spaces/{space_id}/members/{principal_id}",
            delete(revoke_member),
        )
        .route("/spaces/{space_id}/sql-sessions", post(create_sql_session))
        .route(
            "/spaces/{space_id}/sql-sessions/{session_id}",
            get(get_sql_session),
        )
        .route(
            "/spaces/{space_id}/sql-sessions/{session_id}/count",
            get(get_sql_session_count),
        )
        .route(
            "/spaces/{space_id}/sql-sessions/{session_id}/rows",
            get(get_sql_session_rows),
        )
        .route(
            "/spaces/{space_id}/entries",
            get(list_entries).post(create_entry),
        )
        .route("/spaces/{space_id}/entries/options", get(entry_options))
        .route(
            "/spaces/{space_id}/entries/{entry_id}",
            get(get_entry).put(update_entry).delete(delete_entry),
        )
        .route(
            "/spaces/{space_id}/entries/{entry_id}/history",
            get(entry_history),
        )
        .route(
            "/spaces/{space_id}/entries/{entry_id}/history/{revision_id}",
            get(entry_revision),
        )
        .route(
            "/spaces/{space_id}/entries/{entry_id}/restore",
            post(restore_entry),
        )
        .route(
            "/spaces/{space_id}/forms",
            get(list_forms).post(upsert_form),
        )
        .route("/spaces/{space_id}/forms/types", get(form_types))
        .route("/spaces/{space_id}/forms/{form_name}", get(get_form))
        .route("/spaces/{space_id}/search", get(search_entries))
        .route("/spaces/{space_id}/query", post(query_entries))
        .route("/spaces/{space_id}/sql", get(list_sql).post(create_sql))
        .route(
            "/spaces/{space_id}/sql/{sql_id}",
            get(get_sql).put(update_sql).delete(delete_sql),
        )
        .route("/spaces/{space_id}/assets", post(upload_asset))
        .route(
            "/spaces/{space_id}/assets/{asset_id}",
            get(get_asset).delete(delete_asset),
        )
        .route("/mcp/resources/{space_id}/entries/list", get(mcp_entries))
        .route_layer(middleware::from_fn_with_state(state, require_auth))
}

fn api_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/openapi.json", get(|| async { OPENAPI_JSON }))
        .route("/auth/config", get(auth_config))
        .route("/auth/setup/start", post(auth_setup_start))
        .route("/auth/setup/finish", post(auth_setup_finish))
        .route("/auth/passkey/start", post(auth_passkey_start))
        .route("/auth/passkey/finish", post(auth_passkey_finish))
        .route("/auth/invitations/start", post(auth_invitation_start))
        .route("/auth/invitations/finish", post(auth_invitation_finish))
        .route("/auth/recovery/start", post(auth_recovery_start))
        .route("/auth/recovery/finish", post(auth_recovery_finish))
        .route(
            "/auth/recovery/owner/start",
            post(auth_owner_recovery_start),
        )
        .route(
            "/auth/recovery/owner/finish",
            post(auth_owner_recovery_finish),
        )
        .route("/auth/oidc/{provider_id}/start", get(oidc_start))
        .route("/auth/oidc/providers", get(list_oidc_providers))
        .route("/auth/oidc/callback", get(oidc_callback))
        .route(
            "/oauth/device/authorization",
            post(oauth_device_authorization),
        )
        .route("/oauth/token", post(oauth_token))
        .route("/oauth/revoke", post(oauth_revoke))
        .route("/oauth/agent/token", post(issue_autonomous_agent_token))
        .route(
            "/auth/session",
            get(auth_session).delete(auth_session_delete),
        )
        .merge(protected_routes(state))
        .fallback(api_not_found)
}

fn app_layers(router: Router<AppState>, state: AppState) -> Router {
    let mut router = router
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            validate_unsafe_origin,
        ));
    if let Ok(origins) = env::var("UGOITE_CORS_ALLOWED_ORIGINS") {
        let allowed_origins = origins
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| {
                origin
                    .parse::<HeaderValue>()
                    .expect("UGOITE_CORS_ALLOWED_ORIGINS contains an invalid origin")
            })
            .collect::<Vec<_>>();
        if !allowed_origins.is_empty() {
            router = router.layer(
                CorsLayer::new()
                    .allow_origin(AllowOrigin::list(allowed_origins))
                    .allow_credentials(true)
                    .allow_methods([
                        Method::GET,
                        Method::POST,
                        Method::PUT,
                        Method::PATCH,
                        Method::DELETE,
                        Method::OPTIONS,
                    ])
                    .allow_headers([
                        header::ACCEPT,
                        header::AUTHORIZATION,
                        header::CONTENT_TYPE,
                        HeaderName::from_static("idempotency-key"),
                        HeaderName::from_static("dpop"),
                        HeaderName::from_static("x-request-id"),
                    ]),
            );
        }
    }
    router.with_state(state)
}

async fn validate_unsafe_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        let has_browser_session = request.headers().contains_key(header::COOKIE);
        if has_browser_session {
            let valid = request
                .headers()
                .get(header::ORIGIN)
                .and_then(|origin| origin.to_str().ok())
                .is_some_and(|origin| origin == state.identity.public_origin());
            if !valid {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({"code":"ORIGIN_MISMATCH","message":"unsafe browser requests require the canonical Origin"})),
                )
                    .into_response();
            }
        }
    }
    next.run(request).await
}

pub fn app(state: AppState) -> Router {
    let metadata = Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth_protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server_metadata),
        );
    let router = if let Ok(static_dir) = env::var("UGOITE_STATIC_DIR") {
        metadata
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/openapi.json", get(|| async { OPENAPI_JSON }))
            .route_service("/", ServeFile::new(format!("{static_dir}/index.html")))
            .nest("/api", api_routes(state.clone()))
            .fallback_service(
                ServeDir::new(&static_dir)
                    .fallback(ServeFile::new(format!("{static_dir}/index.html"))),
            )
    } else {
        metadata.merge(api_routes(state.clone())).route(
            "/",
            get(|| async { Json(json!({"message": "Hello World!"})) }),
        )
    };
    app_layers(router, state)
}

async fn require_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request
        .uri()
        .path()
        .strip_prefix("/api")
        .unwrap_or(request.uri().path());
    let setup_strengthening = path == "/auth/session"
        || path.starts_with("/auth/passkeys")
        || path.starts_with("/auth/recovery/totp");
    if !state.identity.node_is_active().await.unwrap_or(false) && !setup_strengthening {
        return (
            StatusCode::LOCKED,
            Json(json!({
                "code": "NODE_UNINITIALIZED",
                "message": "complete node setup before using Ugoite APIs"
            })),
        )
            .into_response();
    }
    let identity = if let Some(session_id) = auth_session_cookie(&headers) {
        match state.identity.authenticate_session(&session_id).await {
            Ok(authenticated) => RequestIdentityContext {
                request_identity: RequestIdentity {
                    subject: AuthenticatedSubject::HumanAccount {
                        account_id: authenticated.account.account_id,
                    },
                    actor: Actor::Human {
                        account_id: authenticated.account.account_id,
                    },
                    credential_id: authenticated.credential_id,
                    authentication_method: if matches!(
                        authenticated.assurance,
                        AssuranceLevel::Federated
                    ) {
                        RequestAuthenticationMethod::Oidc
                    } else {
                        RequestAuthenticationMethod::Passkey
                    },
                    assurance: authenticated.assurance,
                    constraints: CredentialConstraints::default(),
                    session_id: Some(authenticated.session_id),
                },
                account_id: authenticated.account.account_id,
                display_name: authenticated.account.display_name,
                node_admin: authenticated
                    .account
                    .node_roles
                    .contains(&NodeRole::NodeAdmin),
                token_principal_id: None,
                token_actor_principal_id: None,
                token_space_uid: None,
                token_actions: None,
                recent_passkey: state
                    .identity
                    .session_has_recent_passkey(&session_id)
                    .await
                    .unwrap_or(false),
                credential_generation: authenticated.account.credential_generation,
            },
            Err(_) => return unauthorized("session is invalid or expired"),
        }
    } else {
        let Some(access_token) = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("DPoP "))
        else {
            return unauthorized("a valid session or DPoP access token is required");
        };
        let Some(proof) = headers.get("dpop").and_then(|value| value.to_str().ok()) else {
            return unauthorized("DPoP proof header is required");
        };
        let Ok(claims) = state.identity.resolve_access_credential(access_token).await else {
            return unauthorized("access token is invalid");
        };
        let Ok((issuer, node_id)) = state.identity.issuer_metadata().await else {
            return unauthorized("token issuer unavailable");
        };
        if claims.aud != issuer || claims.node_id != node_id {
            return unauthorized("access token audience is invalid");
        }
        let agent_credential =
            claims.principal_type == "agent" || claims.actor_principal_id.is_some();
        let (account_id, display_name, proof_key) = if agent_credential {
            let Ok(credential) = state.identity.agent_credential(claims.credential_id).await else {
                return unauthorized("agent credential is revoked");
            };
            let expected_agent_id = access_token_agent_id(&claims);
            if &credential.agent_id != expected_agent_id {
                return unauthorized("agent credential subject mismatch");
            }
            (Uuid::nil(), "Agent".to_string(), credential.public_key_jwk)
        } else {
            let Ok(credential) = state.identity.device_credential(claims.credential_id).await
            else {
                return unauthorized("device credential is revoked");
            };
            (
                credential.account_id,
                credential.device_name,
                credential.public_key_jwk,
            )
        };
        let Ok(thumbprint) = oauth::jwk_thumbprint(&proof_key) else {
            return unauthorized("proof key is invalid");
        };
        if thumbprint != claims.cnf.jkt {
            return unauthorized("access token is not bound to this proof key");
        }
        let htu = format!(
            "{}{}",
            issuer.trim_end_matches('/'),
            request
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or(request.uri().path())
        );
        let Ok(proof_claims) = oauth::verify_dpop_proof(
            proof,
            &proof_key,
            request.method().as_str(),
            &htu,
            access_token,
        ) else {
            return unauthorized("DPoP proof is invalid");
        };
        if state
            .identity
            .record_proof_jti(&proof_claims.jti)
            .await
            .is_err()
        {
            return unauthorized("DPoP proof was replayed");
        }
        RequestIdentityContext {
            request_identity: RequestIdentity {
                subject: if claims.principal_type == "agent" {
                    AuthenticatedSubject::AgentPrincipal {
                        agent_id: claims.sub,
                    }
                } else if claims.actor_principal_id.is_some() {
                    AuthenticatedSubject::SpacePrincipal {
                        principal_id: claims.sub,
                    }
                } else {
                    AuthenticatedSubject::HumanAccount { account_id }
                },
                actor: if let Some(agent_id) = claims.actor_principal_id {
                    Actor::Agent { agent_id }
                } else if claims.principal_type == "agent" {
                    Actor::Agent {
                        agent_id: claims.sub,
                    }
                } else {
                    Actor::CliDevice {
                        credential_id: claims.credential_id,
                    }
                },
                credential_id: claims.credential_id,
                authentication_method: if claims.principal_type == "agent" {
                    RequestAuthenticationMethod::AgentAssertion
                } else {
                    RequestAuthenticationMethod::DeviceProof
                },
                assurance: AssuranceLevel::Possession,
                constraints: CredentialConstraints {
                    issuer: Some(claims.iss.clone()),
                    node_id: Some(claims.node_id),
                    audience: Some(claims.aud.clone()),
                    space_id: Some(claims.space_uid),
                    actions: claims
                        .granted_actions
                        .iter()
                        .filter_map(|action| parse_action(action).ok())
                        .collect(),
                    expires_at: chrono::DateTime::from_timestamp(claims.exp, 0)
                        .map(|value| value.to_rfc3339()),
                    confirmation_key_thumbprint: Some(claims.cnf.jkt.clone()),
                },
                session_id: None,
            },
            account_id,
            display_name,
            node_admin: false,
            token_principal_id: Some(claims.sub),
            token_actor_principal_id: claims.actor_principal_id,
            token_space_uid: Some(claims.space_uid),
            token_actions: Some(claims.granted_actions),
            recent_passkey: false,
            credential_generation: claims.credential_generation.unwrap_or_default(),
        }
    };
    request.extensions_mut().insert(identity);
    next.run(request).await
}

fn unauthorized(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"detail": message}))).into_response()
}

fn require_recent_passkey(identity: &RequestIdentityContext) -> ApiResult<()> {
    if identity.token_principal_id.is_some()
        || !identity.recent_passkey
        || !matches!(
            identity.request_identity.assurance,
            AssuranceLevel::PhishingResistant
        )
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            json!({"code":"RECENT_PASSKEY_REQUIRED","message":"repeat Passkey authentication within five minutes"}),
        ));
    }
    Ok(())
}

async fn auth_config(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    state
        .identity
        .state_summary()
        .await
        .map(Json)
        .map_err(auth_error)
}

async fn api_not_found(OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    if uri.path().starts_with("/api//") {
        return (StatusCode::BAD_REQUEST, "Invalid API proxy path").into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"detail": "API route not found"})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct SetupStartRequest {
    setup_secret: String,
    display_name: String,
}

async fn auth_setup_start(
    State(state): State<AppState>,
    Json(payload): Json<SetupStartRequest>,
) -> ApiResult<Json<Value>> {
    let result = state
        .identity
        .start_setup_registration(&payload.setup_secret, &payload.display_name)
        .await
        .map_err(auth_error)?;
    Ok(Json(
        serde_json::to_value(result).map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct SetupFinishRequest {
    setup_secret: String,
    challenge_id: Uuid,
    credential: RegisterPublicKeyCredential,
}

async fn auth_setup_finish(
    State(state): State<AppState>,
    Json(payload): Json<SetupFinishRequest>,
) -> ApiResult<Response> {
    // Validate every existing Space before the identity service consumes the
    // one-time setup secret and persists the new account. Current-release
    // setup does not upgrade old Space layouts, so an invalid Space must leave
    // setup retryable with the original secret.
    let existing_spaces = state
        .service
        .list_space_ids()
        .await
        .map_err(ApiError::from_core)?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    for space_id in &existing_spaces {
        let space_uid = state
            .service
            .space_uid(space_id)
            .await
            .map_err(ApiError::from_core)?;
        authorizer
            .validate_current_layout(space_id, space_uid)
            .await
            .map_err(ApiError::from_core)?;
    }
    let result = state
        .identity
        .finish_setup_registration(
            &payload.setup_secret,
            payload.challenge_id,
            &payload.credential,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    let mut claimed_space_uids = Vec::new();
    if existing_spaces.is_empty() {
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("default", principal_id, &result.account.display_name)
            .await
            .map_err(ApiError::from_core)?;
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid,
                principal_id,
                node_account_id: result.account.account_id,
                binding_method: BindingMethod::Setup,
            })
            .await
            .map_err(auth_error)?;
        claimed_space_uids.push(space_uid);
    } else {
        for space_id in existing_spaces {
            let space_uid = state
                .service
                .space_uid(&space_id)
                .await
                .map_err(ApiError::from_core)?;
            let principal_id = authorizer
                .ensure_owner(&space_id, space_uid, &result.account.display_name)
                .await
                .map_err(ApiError::from_core)?;
            state
                .identity
                .add_binding(ugoite_domain::identity::PrincipalBinding {
                    space_uid,
                    principal_id,
                    node_account_id: result.account.account_id,
                    binding_method: BindingMethod::Setup,
                })
                .await
                .map_err(auth_error)?;
            claimed_space_uids.push(space_uid);
        }
    }
    let space_uid = claimed_space_uids.first().copied();
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(result.account.account_id),
            actor_account_id: Some(result.account.account_id),
            credential_id: None,
            action: "node.setup.completed",
            target_type: "node",
            target_id: None,
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"claimed_space_count": claimed_space_uids.len()}),
        })
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        [(
            "set-cookie",
            auth_cookie(&result.session_id, 60 * 60 * 24 * 30),
        )],
        Json(json!({
            "account": result.account,
            "space_uid": space_uid,
            "claimed_space_uids": claimed_space_uids,
            "recovery_codes": result.recovery_codes
        })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct InvitationStartRequest {
    invitation_token: String,
}

async fn auth_invitation_start(
    State(state): State<AppState>,
    Json(payload): Json<InvitationStartRequest>,
) -> ApiResult<Json<Value>> {
    reconcile_all_recovery_fences(&state)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let result = state
        .identity
        .start_invitation_registration(&payload.invitation_token)
        .await
        .map_err(auth_error)?;
    Ok(Json(
        serde_json::to_value(result).map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct InvitationFinishRequest {
    invitation_token: String,
    challenge_id: Uuid,
    credential: RegisterPublicKeyCredential,
}

async fn auth_invitation_finish(
    State(state): State<AppState>,
    Json(payload): Json<InvitationFinishRequest>,
) -> ApiResult<Response> {
    reconcile_all_recovery_fences(&state)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let result = state
        .identity
        .finish_invitation_registration(
            &payload.invitation_token,
            payload.challenge_id,
            &payload.credential,
        )
        .await
        .map_err(auth_error)?;
    bind_invited_account(
        &state,
        &result.account,
        &result.invitation,
        BindingMethod::Invite,
    )
    .await?;
    state
        .identity
        .complete_invitation_acceptance(
            result.invitation.invitation_id,
            result.account.account_id,
            result.invitation.accepted_principal_id().ok_or_else(|| {
                ApiError::new(StatusCode::CONFLICT, "invitation acceptance is incomplete")
            })?,
        )
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        [(
            "set-cookie",
            auth_cookie(&result.session_id, 60 * 60 * 24 * 30),
        )],
        Json(json!({"account": result.account})),
    )
        .into_response())
}

async fn auth_invitation_accept_existing(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<InvitationStartRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    reconcile_all_recovery_fences(&state)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let (account, invitation) = state
        .identity
        .accept_invitation_for_account(&payload.invitation_token, identity.account_id)
        .await
        .map_err(auth_error)?;
    bind_invited_account(&state, &account, &invitation, BindingMethod::Invite).await?;
    state
        .identity
        .complete_invitation_acceptance(
            invitation.invitation_id,
            account.account_id,
            invitation.accepted_principal_id().ok_or_else(|| {
                ApiError::new(StatusCode::CONFLICT, "invitation acceptance is incomplete")
            })?,
        )
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "account": account,
            "space_uid": invitation.space_uid,
            "principal_id": invitation.accepted_principal_id(),
        })),
    ))
}

async fn auth_passkey_start(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let result = state
        .identity
        .start_authentication()
        .await
        .map_err(auth_error)?;
    Ok(Json(
        serde_json::to_value(result).map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct PasskeyFinishRequest {
    challenge_id: Uuid,
    credential: PublicKeyCredential,
}

async fn auth_passkey_finish(
    State(state): State<AppState>,
    Json(payload): Json<PasskeyFinishRequest>,
) -> ApiResult<Response> {
    let (account, session_id) = state
        .identity
        .finish_authentication(payload.challenge_id, &payload.credential)
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::OK,
        [("set-cookie", auth_cookie(&session_id, 60 * 60 * 24 * 30))],
        Json(json!({"account": account})),
    )
        .into_response())
}

async fn list_passkeys(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    Ok(Json(Value::Array(
        state
            .identity
            .list_passkeys(identity.account_id)
            .await
            .map_err(auth_error)?,
    )))
}

async fn list_sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    let sessions = state
        .identity
        .list_sessions(identity.account_id)
        .await
        .map_err(auth_error)?;
    Ok(Json(Value::Array(sessions)))
}

async fn revoke_session_by_id(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(session_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .identity
        .revoke_session_by_id(identity.account_id, session_id)
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(identity.request_identity.credential_id),
            action: "session.revoked",
            target_type: "browser_session",
            target_id: Some(session_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn start_add_passkey(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    Ok(Json(
        serde_json::to_value(
            state
                .identity
                .start_add_passkey(identity.account_id)
                .await
                .map_err(recovery_aware_auth_error)?,
        )
        .map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct AddPasskeyFinishRequest {
    challenge_id: Uuid,
    credential: RegisterPublicKeyCredential,
}

async fn finish_add_passkey(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<AddPasskeyFinishRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_recent_passkey(&identity)?;
    let result = state
        .identity
        .finish_add_passkey(
            identity.account_id,
            payload.challenge_id,
            &payload.credential,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "passkey.added",
            target_type: "passkey",
            target_id: result
                .get("credential_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn revoke_passkey(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(credential_id): Path<String>,
) -> ApiResult<StatusCode> {
    require_recent_passkey(&identity)?;
    state
        .identity
        .revoke_passkey(
            identity.account_id,
            identity.credential_generation,
            &credential_id,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "passkey.removed",
            target_type: "passkey",
            target_id: Some(credential_id),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_node_audit_events(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "node admin role is required",
        ));
    }
    Ok(Json(
        serde_json::to_value(
            state
                .identity
                .list_node_audit(200)
                .await
                .map_err(auth_error)?,
        )
        .map_err(|error| auth_error(error.into()))?,
    ))
}

async fn start_totp_enrollment(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    state
        .identity
        .start_totp_enrollment(identity.account_id)
        .await
        .map(Json)
        .map_err(recovery_aware_auth_error)
}

#[derive(Deserialize)]
struct TotpFinishRequest {
    code: String,
}

async fn finish_totp_enrollment(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<TotpFinishRequest>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    match state
        .identity
        .finish_totp_enrollment(identity.account_id, &payload.code)
        .await
    {
        Ok(()) => {}
        Err(TotpEnrollmentFinishError::InvalidOrExpired) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                json!({
                    "code": "INVALID_TOTP",
                    "message": "invalid or expired TOTP enrollment code"
                }),
            ));
        }
        Err(TotpEnrollmentFinishError::Internal(error))
            if error.to_string().contains("RECOVERY_FENCE_UNAVAILABLE") =>
        {
            return Err(recovery_fence_unavailable());
        }
        Err(TotpEnrollmentFinishError::Internal(_error)) => {
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({
                    "code": "TOTP_ENROLLMENT_FAILED",
                    "message": "TOTP enrollment failed"
                }),
            ));
        }
    }
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(identity.request_identity.credential_id),
            action: "recovery.totp_configured",
            target_type: "human_account",
            target_id: Some(identity.account_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok(Json(json!({"configured": true})))
}

#[derive(Deserialize)]
struct RecoveryStartRequest {
    account_id: Uuid,
    recovery_code: String,
    totp_code: String,
}

async fn auth_recovery_start(
    State(state): State<AppState>,
    Json(payload): Json<RecoveryStartRequest>,
) -> ApiResult<Json<Value>> {
    let start = state
        .identity
        .start_recovery_registration(
            payload.account_id,
            &payload.recovery_code,
            &payload.totp_code,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok(Json(
        serde_json::to_value(start).map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct RecoveryFinishRequest {
    account_id: Uuid,
    challenge_id: Uuid,
    credential: RegisterPublicKeyCredential,
}

async fn auth_recovery_finish(
    State(state): State<AppState>,
    Json(payload): Json<RecoveryFinishRequest>,
) -> ApiResult<Response> {
    let result = state
        .identity
        .finish_recovery_registration(
            payload.account_id,
            payload.challenge_id,
            &payload.credential,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok((
        StatusCode::CREATED,
        [(
            "set-cookie",
            auth_cookie(&result.session_id, 60 * 60 * 24 * 30),
        )],
        Json(json!({
            "account": result.account,
            "recovery_codes": result.recovery_codes
        })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct OwnerRecoveryApprovalRequest {
    principal_id: Uuid,
}

#[derive(Deserialize)]
struct OwnerRecoveryStartRequest {
    owner_approval_token: String,
}

#[derive(Deserialize)]
struct OwnerRecoveryFinishRequest {
    challenge_id: Uuid,
    credential: RegisterPublicKeyCredential,
}

async fn recovery_owner_context(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
) -> ApiResult<(Uuid, Uuid)> {
    validate_id(space_id, "space_id")?;
    require_recent_passkey(identity).map_err(|_| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "an active Space Owner browser session with a recent Passkey is required"
            }),
        )
    })?;
    if identity.token_principal_id.is_some()
        || identity.token_actor_principal_id.is_some()
        || !matches!(
            identity.request_identity.subject,
            AuthenticatedSubject::HumanAccount { .. }
        )
        || !matches!(
            identity.request_identity.authentication_method,
            RequestAuthenticationMethod::Passkey
        )
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "owner recovery requires a browser Passkey session"
            }),
        ));
    }
    let space_uid = state
        .service
        .space_uid(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let caller_principal = principal_for_space(state, space_id, identity)
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                json!({
                    "code": "RECOVERY_AUTHORITY_REQUIRED",
                    "message": "Space Owner authority is required"
                }),
            )
        })?;
    let owner_principal = authorization
        .memberships
        .get(&caller_principal)
        .filter(|membership| matches!(membership.role, SpaceRole::Owner))
        .map(|membership| membership.principal_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                json!({
                    "code": "RECOVERY_AUTHORITY_REQUIRED",
                    "message": "Space Owner authority is required"
                }),
            )
        })?;
    let principal = authorization
        .principals
        .get(&owner_principal)
        .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "Space Owner is unavailable"))?;
    if !matches!(principal.kind, PrincipalKind::Human)
        || !matches!(principal.state, PrincipalState::Active)
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "Space Owner authority is required"
            }),
        ));
    }
    Ok((space_uid, owner_principal))
}

async fn recovery_target_account(
    state: &AppState,
    space_id: &str,
    space_uid: Uuid,
    principal_id: Uuid,
    issuer_account_id: Uuid,
) -> ApiResult<Uuid> {
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let principal = authorization
        .principals
        .get(&principal_id)
        .filter(|principal| {
            matches!(principal.kind, PrincipalKind::Human)
                && matches!(principal.state, PrincipalState::Active)
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
            )
        })?;
    if !authorization
        .memberships
        .contains_key(&principal.principal_id)
    {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
        ));
    }
    let bindings = state
        .identity
        .bindings_for_space(space_uid)
        .await
        .map_err(auth_error)?;
    let matching = bindings
        .iter()
        .filter(|binding| binding.principal_id == principal_id)
        .collect::<Vec<_>>();
    if matching.len() != 1 || matching[0].node_account_id == issuer_account_id {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
        ));
    }
    let account_id = matching[0].node_account_id;
    let account = state
        .identity
        .list_accounts()
        .await
        .map_err(auth_error)?
        .into_iter()
        .find(|account| account.account_id == account_id);
    if account.is_none_or(|account| !matches!(account.status, AccountStatus::Active)) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
        ));
    }
    Ok(account_id)
}

async fn validate_owner_recovery_context(
    state: &AppState,
    context: &OwnerRecoveryContext,
) -> ApiResult<()> {
    if context.account_id == context.issuer_account_id
        || context.principal_id == context.issuer_principal_id
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
        ));
    }
    let space_id = find_space_id_by_uid(state, context.space_uid).await?;
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let fence = context
        .recovery_fence_id
        .and_then(|fence_id| authorization.recovery_fences.get(&fence_id))
        .ok_or_else(|| recovery_fence_unavailable())?;
    let fence_expires_at = chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
        .map(|expires| expires.with_timezone(&chrono::Utc))
        .map_err(|_| recovery_storage_unavailable())?;
    if fence.status != "active"
        || fence_expires_at <= chrono::Utc::now()
        || fence.space_uid != context.space_uid
        || fence.issuer_principal_id != context.issuer_principal_id
        || fence.issuer_account_id != context.issuer_account_id
        || fence.target_principal_id != context.principal_id
        || fence.target_account_id != context.account_id
        || fence.authorization_revision != context.space_authorization_revision
        || fence.issuer_space_lifecycle_epoch != context.issuer_space_lifecycle_epoch
        || fence.target_space_lifecycle_epoch != context.target_space_lifecycle_epoch
        || fence.issuer_generation != context.issuer_generation
        || fence.target_generation != context.target_generation
    {
        return Err(recovery_fence_unavailable());
    }
    if authorization
        .principal_lifecycle_epochs
        .get(&context.issuer_principal_id)
        .copied()
        != Some(context.issuer_space_lifecycle_epoch)
        || authorization
            .principal_lifecycle_epochs
            .get(&context.principal_id)
            .copied()
            != Some(context.target_space_lifecycle_epoch)
    {
        return Err(recovery_fence_unavailable());
    }
    let issuer = authorization
        .principals
        .get(&context.issuer_principal_id)
        .filter(|principal| {
            matches!(principal.kind, PrincipalKind::Human)
                && matches!(principal.state, PrincipalState::Active)
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
            )
        })?;
    if !authorization
        .memberships
        .get(&issuer.principal_id)
        .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
    {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
        ));
    }
    let target = authorization
        .principals
        .get(&context.principal_id)
        .filter(|principal| {
            matches!(principal.kind, PrincipalKind::Human)
                && matches!(principal.state, PrincipalState::Active)
        })
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
            )
        })?;
    if !authorization.memberships.contains_key(&target.principal_id) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
        ));
    }
    let bindings = state
        .identity
        .bindings_for_space(context.space_uid)
        .await
        .map_err(auth_error)?;
    let issuer_binding_count = bindings
        .iter()
        .filter(|binding| {
            binding.principal_id == context.issuer_principal_id
                && binding.node_account_id == context.issuer_account_id
        })
        .count();
    let target_binding_count = bindings
        .iter()
        .filter(|binding| {
            binding.principal_id == context.principal_id
                && binding.node_account_id == context.account_id
        })
        .count();
    if issuer_binding_count != 1 || target_binding_count != 1 {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
        ));
    }
    let issuer_node_lifecycle_epoch = state
        .identity
        .recovery_account_lifecycle_epoch(context.issuer_account_id)
        .await
        .map_err(auth_error)?;
    let target_node_lifecycle_epoch = state
        .identity
        .recovery_account_lifecycle_epoch(context.account_id)
        .await
        .map_err(auth_error)?;
    if issuer_node_lifecycle_epoch != context.issuer_node_lifecycle_epoch
        || target_node_lifecycle_epoch != context.target_node_lifecycle_epoch
    {
        return Err(recovery_fence_unavailable());
    }
    let accounts = state.identity.list_accounts().await.map_err(auth_error)?;
    let target_account = accounts
        .iter()
        .find(|account| account.account_id == context.account_id)
        .filter(|account| {
            matches!(account.status, AccountStatus::Active)
                && account.credential_generation == context.target_generation
        });
    let issuer_account = accounts
        .iter()
        .find(|account| account.account_id == context.issuer_account_id)
        .filter(|account| {
            matches!(account.status, AccountStatus::Active)
                && account.credential_generation == context.issuer_generation
        });
    if target_account.is_none() || issuer_account.is_none() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            json!({"code":"AUTHENTICATION_REQUIRED","message":"owner recovery is invalid"}),
        ));
    }
    Ok(())
}

fn recovery_fence_unavailable() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        json!({
            "code": "RECOVERY_FENCE_UNAVAILABLE",
            "message": "recovery could not obtain a durable authorization fence"
        }),
    )
}

fn has_active_recovery_fence(authorization: &AuthorizationState) -> bool {
    authorization
        .recovery_fences
        .values()
        .any(|fence| fence.status == "active")
}

fn recovery_storage_unavailable() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({
            "code": "RECOVERY_STORAGE_UNAVAILABLE",
            "message": "recovery state could not be durably committed"
        }),
    )
}

async fn recovery_account_generations(
    state: &AppState,
    issuer_account_id: Uuid,
    target_account_id: Uuid,
) -> ApiResult<(u64, u64)> {
    let accounts = state.identity.list_accounts().await.map_err(auth_error)?;
    let issuer_generation = accounts
        .iter()
        .find(|account| account.account_id == issuer_account_id)
        .map(|account| account.credential_generation)
        .ok_or_else(recovery_fence_unavailable)?;
    let target_generation = accounts
        .iter()
        .find(|account| account.account_id == target_account_id)
        .map(|account| account.credential_generation)
        .ok_or_else(recovery_fence_unavailable)?;
    Ok((issuer_generation, target_generation))
}

/// Acquire the Node half before publishing the Space fence. This closes the
/// cross-store interval in which an invitation or credential mutation could
/// commit after the Space barrier was visible but before Node had recorded its
/// barrier. The second Node acquisition revalidates the Space snapshot after
/// reservation, so a concurrent Space change fails closed.
async fn reserve_recovery_pair(
    state: &AppState,
    space_id: &str,
    space_uid: Uuid,
    issuer_principal_id: Uuid,
    issuer_account_id: Uuid,
    target_principal_id: Uuid,
    target_account_id: Uuid,
    request_id: Uuid,
    ttl: chrono::Duration,
) -> ApiResult<(
    Authorizer,
    ugoite_iceberg::authorization::RecoveryFence,
    RecoveryBindingSnapshot,
)> {
    let (issuer_generation, target_generation) =
        recovery_account_generations(state, issuer_account_id, target_account_id).await?;
    let issuer_node_epoch = state
        .identity
        .recovery_account_lifecycle_epoch(issuer_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let target_node_epoch = state
        .identity
        .recovery_account_lifecycle_epoch(target_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let fence_id = Uuid::now_v7();
    let recovery_fence_expires_at =
        (chrono::Utc::now() + ttl).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let provisional = RecoveryBindingSnapshot {
        request_id,
        recovery_fence_id: fence_id,
        recovery_fence_expires_at: recovery_fence_expires_at.clone(),
        space_authorization_revision: 0,
        issuer_space_lifecycle_epoch: 0,
        target_space_lifecycle_epoch: 0,
        issuer_node_lifecycle_epoch: issuer_node_epoch,
        target_node_lifecycle_epoch: target_node_epoch,
        issuer_generation,
        target_generation,
    };
    if let Err(error) = state
        .identity
        .acquire_recovery_fence(
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
            Some(&provisional),
        )
        .await
    {
        return Err(recovery_commit_error(error));
    }
    let authorizer = Authorizer::new(state.service.operator().clone());
    let fence = match authorizer
        .reserve_recovery_fence_with_id(
            space_id,
            request_id,
            fence_id,
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            issuer_generation,
            target_generation,
            ttl,
        )
        .await
    {
        Ok(fence) => fence,
        Err(error) => {
            let _ = state.identity.release_recovery_fence(fence_id).await;
            return Err(recovery_reservation_error(error));
        }
    };
    let snapshot = RecoveryBindingSnapshot {
        request_id,
        recovery_fence_id: fence.fence_id,
        recovery_fence_expires_at: fence.expires_at.clone(),
        space_authorization_revision: fence.authorization_revision,
        issuer_space_lifecycle_epoch: fence.issuer_space_lifecycle_epoch,
        target_space_lifecycle_epoch: fence.target_space_lifecycle_epoch,
        issuer_node_lifecycle_epoch: issuer_node_epoch,
        target_node_lifecycle_epoch: target_node_epoch,
        issuer_generation,
        target_generation,
    };
    if let Err(error) = state
        .identity
        .acquire_recovery_fence(
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
            Some(&snapshot),
        )
        .await
    {
        let _ = authorizer
            .release_recovery_fence(space_id, fence.fence_id)
            .await;
        let _ = state.identity.release_recovery_fence(fence.fence_id).await;
        return Err(recovery_commit_error(error));
    }
    Ok((authorizer, fence, snapshot))
}

fn recovery_reservation_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    let lower = message.to_lowercase();
    if message.contains("RECOVERY_FENCE_UNAVAILABLE") {
        return recovery_fence_unavailable();
    }
    if lower.contains("conflict") || lower.contains("version conflict") {
        return recovery_fence_unavailable();
    }
    if lower.contains("read")
        || lower.contains("write")
        || lower.contains("storage")
        || lower.contains("control-store")
        || lower.contains("control object")
        || lower.contains("authorization state")
        || lower.contains("timestamp")
    {
        return recovery_storage_unavailable();
    }
    ApiError::new(
        StatusCode::NOT_FOUND,
        json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
    )
}

fn recovery_commit_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string().to_lowercase();
    if message.contains("recovery fence")
        || message.contains("recovery tuple is stale")
        || message.contains("node control write committed")
        || message.contains("node control write outcome unknown")
    {
        return recovery_fence_unavailable();
    }
    if message.contains("conflict")
        || message.contains("compare-and-swap")
        || message.contains("control-store")
        || message.contains("storage")
    {
        return recovery_storage_unavailable();
    }
    ApiError::new(
        StatusCode::NOT_FOUND,
        json!({"code":"RECOVERY_TARGET_INVALID","message":"recovery target is invalid"}),
    )
}

async fn reconcile_recovery_fences(state: &AppState, space_id: &str) -> anyhow::Result<()> {
    let space_uid = state.service.space_uid(space_id).await?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let pending_ids = state.identity.pending_recovery_fence_ids(space_uid).await?;
    for fence_id in pending_ids.iter().copied() {
        let fence = match authorizer.recovery_fence(space_id, fence_id).await {
            Ok(fence) => fence,
            Err(_) => continue,
        };
        let space_fence_ready = match fence.status.as_str() {
            "active" => authorizer
                .complete_recovery_fence(space_id, fence_id)
                .await
                .is_ok(),
            "completed" => true,
            // A Node mutation is durable once it appears in pending_ids. Never
            // mark that mutation reconciled merely because its Space fence was
            // released or superseded; that would manufacture a cross-store
            // completion without a completed authorization fence.
            _ => false,
        };
        if !space_fence_ready {
            continue;
        }
        if state
            .identity
            .complete_recovery_fence(fence_id)
            .await
            .is_err()
        {
            continue;
        }
        state
            .identity
            .mark_recovery_fence_reconciled(fence_id)
            .await?;
    }

    let authorization = authorizer.state(space_id).await?;
    let now = chrono::Utc::now();
    let mut expired_space_fences = Vec::new();
    for fence in authorization.recovery_fences.values() {
        if fence.status != "active" || pending_ids.contains(&fence.fence_id) {
            continue;
        }
        let expires_at = chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
            .map(|expires| expires.with_timezone(&chrono::Utc))
            .map_err(|_| anyhow::anyhow!("invalid stored recovery fence timestamp"))?;
        if expires_at <= now {
            expired_space_fences.push(fence.fence_id);
        }
    }
    for fence_id in expired_space_fences {
        // An expired approval without a Node commit is explicitly aborted
        // here. Expiry alone never releases a write barrier.
        authorizer
            .release_recovery_fence(space_id, fence_id)
            .await?;
        state.identity.release_recovery_fence(fence_id).await?;
    }
    for fence_id in state.identity.active_recovery_fence_ids(space_uid).await? {
        if state.identity.expired_recovery_fence(fence_id).await?
            && !pending_ids.contains(&fence_id)
        {
            state.identity.release_recovery_fence(fence_id).await?;
        }
    }
    Ok(())
}

async fn reconcile_all_recovery_fences(state: &AppState) -> anyhow::Result<()> {
    for space_id in state.service.list_space_ids().await? {
        reconcile_recovery_fences(state, &space_id).await?;
    }
    Ok(())
}

async fn abort_owner_recovery_fence(
    state: &AppState,
    space_uid: Uuid,
    fence_id: Uuid,
) -> ApiResult<()> {
    let space_id = find_space_id_by_uid(state, space_uid)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Authorizer::new(state.service.operator().clone())
        .release_recovery_fence(&space_id, fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    state
        .identity
        .release_recovery_fence(fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Ok(())
}

fn recovery_result_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
}

async fn append_recovery_space_audit(state: &AppState, space_id: &str, event_id: Uuid) -> bool {
    let Ok(Some(event)) = state.identity.recovery_audit_event(event_id).await else {
        return false;
    };
    audit::append_audit_event(state.service.operator(), space_id, &event, None)
        .await
        .is_ok()
}

/// Restart-safe recovery audit delivery. The Node outbox contains one
/// redacted canonical event; each transition is persisted before the next
/// delivery attempt, so a crash can resume without replaying secrets.
async fn reconcile_recovery_audit_outbox(state: &AppState, space_id: &str) -> anyhow::Result<()> {
    let space_uid = state.service.space_uid(space_id).await?;
    for record in state
        .identity
        .pending_recovery_audits()
        .await?
        .into_iter()
        .filter(|record| record.space_uid == space_uid)
    {
        if record.status == "pending" {
            state
                .identity
                .append_node_audit_with_id(
                    record.event_id,
                    NodeAuditInput {
                        subject_account_id: Some(record.account_id),
                        actor_account_id: record.issuer_account_id,
                        credential_id: None,
                        action: &record.action,
                        target_type: "human_account",
                        target_id: Some(record.account_id.to_string()),
                        outcome: "success",
                        request_id: Some(record.request_id.to_string()),
                        safe_metadata: record
                            .event
                            .get("metadata")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    },
                )
                .await?;
            state
                .identity
                .mark_recovery_audit_stage(record.event_id, "node")
                .await?;
        }
        let record = state
            .identity
            .pending_recovery_audits()
            .await?
            .into_iter()
            .find(|candidate| candidate.event_id == record.event_id);
        let Some(record) = record else { continue };
        if record.status == "node" {
            audit::append_audit_event(state.service.operator(), space_id, &record.event, None)
                .await?;
            state
                .identity
                .mark_recovery_audit_stage(record.event_id, "space")
                .await?;
        }
        if state
            .identity
            .pending_recovery_audits()
            .await?
            .into_iter()
            .any(|candidate| candidate.event_id == record.event_id && candidate.status == "space")
        {
            state
                .identity
                .mark_recovery_audit_delivered(record.event_id)
                .await?;
        }
    }
    Ok(())
}

async fn owner_force_reset(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<OwnerRecoveryApprovalRequest>,
) -> ApiResult<Response> {
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let (space_uid, issuer_principal_id) =
        recovery_owner_context(&state, &space_id, &identity).await?;
    let account_id = recovery_target_account(
        &state,
        &space_id,
        space_uid,
        payload.principal_id,
        identity.account_id,
    )
    .await?;
    let request_id = Uuid::now_v7();
    let (authorizer, fence, snapshot) = reserve_recovery_pair(
        &state,
        &space_id,
        space_uid,
        issuer_principal_id,
        identity.account_id,
        payload.principal_id,
        account_id,
        request_id,
        chrono::Duration::minutes(15),
    )
    .await?;
    let (approval_id, token, expires_at) = match state
        .identity
        .issue_owner_recovery_approval_with_snapshot(
            space_uid,
            payload.principal_id,
            account_id,
            issuer_principal_id,
            identity.account_id,
            Some(snapshot),
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let _ = authorizer
                .release_recovery_fence(&space_id, fence.fence_id)
                .await;
            let _ = state.identity.release_recovery_fence(fence.fence_id).await;
            return Err(recovery_commit_error(error));
        }
    };
    let node_audit_status = state
        .identity
        .append_node_audit_with_id(
            approval_id,
            NodeAuditInput {
                subject_account_id: Some(account_id),
                actor_account_id: Some(identity.account_id),
                credential_id: Some(identity.request_identity.credential_id),
                action: "recovery.owner_approval_issued",
                target_type: "human_account",
                target_id: Some(account_id.to_string()),
                outcome: "success",
                request_id: Some(approval_id.to_string()),
                safe_metadata: json!({
                    "space_uid": space_uid,
                    "principal_id": payload.principal_id,
                    "issuer_principal_id": issuer_principal_id
                }),
            },
        )
        .await
        .is_ok();
    if node_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(approval_id, "node")
            .await;
    }
    let space_audit_status = append_recovery_space_audit(&state, &space_id, approval_id).await;
    if space_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(approval_id, "space")
            .await;
    }
    let audit_delivered = if node_audit_status && space_audit_status {
        state
            .identity
            .mark_recovery_audit_delivered(approval_id)
            .await
            .is_ok()
    } else {
        false
    };
    let mut headers = recovery_result_headers();
    Ok((
        StatusCode::CREATED,
        std::mem::take(&mut headers),
        Json(json!({
            "principal_id": payload.principal_id,
            "owner_approval_token": token,
            "expires_at": expires_at,
            "audit_status": if audit_delivered { "delivered" } else { "pending" }
        })),
    )
        .into_response())
}

async fn owner_rotate_backup_codes(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<OwnerRecoveryApprovalRequest>,
) -> ApiResult<Response> {
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let Some(value) = headers.get("idempotency-key") else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"code":"BACKUP_IDEMPOTENCY_KEY_INVALID","message":"Idempotency-Key must be a UUIDv4"}),
        ));
    };
    let request_id = value
        .to_str()
        .ok()
        .and_then(|value| Uuid::parse_str(value.trim()).ok())
        .filter(|value| value.get_version() == Some(Version::Random))
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"code":"BACKUP_IDEMPOTENCY_KEY_INVALID","message":"Idempotency-Key must be a UUIDv4"}),
            )
        })?;
    let (space_uid, issuer_principal_id) =
        recovery_owner_context(&state, &space_id, &identity).await?;
    if let Some(existing) = state
        .identity
        .backup_rotation_request(request_id)
        .await
        .map_err(auth_error)?
    {
        let same_tuple = existing.space_uid == space_uid
            && existing.principal_id == payload.principal_id
            && existing.issuer_principal_id == issuer_principal_id
            && existing.issuer_account_id == identity.account_id;
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            json!({
                "code": if same_tuple { "BACKUP_ROTATION_ALREADY_COMMITTED" } else { "BACKUP_ROTATION_KEY_MISMATCH" },
                "message": if same_tuple { "backup rotation is already committed" } else { "idempotency key is bound to another recovery tuple" }
            }),
        ));
    }
    let account_id = recovery_target_account(
        &state,
        &space_id,
        space_uid,
        payload.principal_id,
        identity.account_id,
    )
    .await?;
    let (authorizer, fence, snapshot) = reserve_recovery_pair(
        &state,
        &space_id,
        space_uid,
        issuer_principal_id,
        identity.account_id,
        payload.principal_id,
        account_id,
        request_id,
        chrono::Duration::minutes(5),
    )
    .await?;
    let codes = match state
        .identity
        .rotate_recovery_codes_with_snapshot(
            request_id,
            space_uid,
            payload.principal_id,
            account_id,
            issuer_principal_id,
            identity.account_id,
            Some(snapshot),
        )
        .await
    {
        Ok(codes) => codes,
        Err(error) => {
            let message = error.to_string();
            if message.contains("node control write committed")
                || message.contains("node control write outcome unknown")
            {
                // The marker may already be durable. Keep both fences active
                // so restart reconciliation can inspect the request marker;
                // releasing here could strand a committed rotation with no
                // way to return its one-time codes safely.
                return Err(recovery_fence_unavailable());
            }
            let _ = authorizer
                .release_recovery_fence(&space_id, fence.fence_id)
                .await;
            let _ = state.identity.release_recovery_fence(fence.fence_id).await;
            if message.contains("key mismatch") {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    json!({"code":"BACKUP_ROTATION_KEY_MISMATCH","message":"idempotency key is bound to another recovery tuple"}),
                ));
            }
            if message.contains("already committed") {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    json!({"code":"BACKUP_ROTATION_ALREADY_COMMITTED","message":"backup rotation is already committed"}),
                ));
            }
            return Err(recovery_commit_error(error));
        }
    };
    let space_fence_committed = authorizer
        .complete_recovery_fence(&space_id, fence.fence_id)
        .await
        .is_ok();
    let node_fence_committed = if space_fence_committed {
        state
            .identity
            .complete_recovery_fence(fence.fence_id)
            .await
            .is_ok()
    } else {
        false
    };
    if !space_fence_committed || !node_fence_committed {
        // The Node mutation is already durable. Do not expose the one-time
        // plaintext codes until both halves of the fence are durable; the
        // restart reconciler will finish the marker without replaying them.
        return Err(recovery_fence_unavailable());
    }
    if space_fence_committed && node_fence_committed {
        let _ = state
            .identity
            .mark_recovery_fence_reconciled(fence.fence_id)
            .await;
    }
    let node_audit_status = state
        .identity
        .append_node_audit_with_id(
            request_id,
            NodeAuditInput {
                subject_account_id: Some(account_id),
                actor_account_id: Some(identity.account_id),
                credential_id: Some(identity.request_identity.credential_id),
                action: "recovery.backup_codes_rotated",
                target_type: "human_account",
                target_id: Some(account_id.to_string()),
                outcome: "success",
                request_id: Some(request_id.to_string()),
                safe_metadata: json!({
                    "space_uid": space_uid,
                    "principal_id": payload.principal_id,
                    "issuer_principal_id": issuer_principal_id,
                    "code_count": codes.len()
                }),
            },
        )
        .await
        .is_ok();
    if node_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(request_id, "node")
            .await;
    }
    let space_audit_status = append_recovery_space_audit(&state, &space_id, request_id).await;
    if space_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(request_id, "space")
            .await;
    }
    let audit_delivered = if node_audit_status && space_audit_status {
        state
            .identity
            .mark_recovery_audit_delivered(request_id)
            .await
            .is_ok()
    } else {
        false
    };
    let mut response_headers = recovery_result_headers();
    Ok((
        StatusCode::OK,
        std::mem::take(&mut response_headers),
        Json(json!({
            "principal_id": payload.principal_id,
            "codes": codes,
            "issued_at": chrono::Utc::now().to_rfc3339(),
            "audit_status": if audit_delivered && space_fence_committed && node_fence_committed { "delivered" } else { "pending" }
        })),
    )
        .into_response())
}

async fn auth_owner_recovery_start(
    State(state): State<AppState>,
    Json(payload): Json<OwnerRecoveryStartRequest>,
) -> ApiResult<Response> {
    reconcile_all_recovery_fences(&state)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let context = match state
        .identity
        .owner_recovery_approval_context(&payload.owner_approval_token)
        .await
    {
        Ok(context) => context,
        Err(error) => {
            if let Some((space_uid, fence_id)) = state
                .identity
                .owner_recovery_abort_fence_for_token(&payload.owner_approval_token)
                .await
                .map_err(|_| recovery_storage_unavailable())?
            {
                abort_owner_recovery_fence(&state, space_uid, fence_id).await?;
            }
            return Err(owner_recovery_api_error(error));
        }
    };
    validate_owner_recovery_context(&state, &context).await?;
    let result = state
        .identity
        .start_owner_recovery_registration(&payload.owner_approval_token)
        .await
        .map_err(owner_recovery_api_error)?;
    let mut headers = recovery_result_headers();
    Ok((
        StatusCode::OK,
        std::mem::take(&mut headers),
        Json(serde_json::to_value(result).map_err(|error| auth_error(error.into()))?),
    )
        .into_response())
}

async fn auth_owner_recovery_finish(
    State(state): State<AppState>,
    Json(payload): Json<OwnerRecoveryFinishRequest>,
) -> ApiResult<Response> {
    reconcile_all_recovery_fences(&state)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let context = match state
        .identity
        .owner_recovery_challenge_context(payload.challenge_id)
        .await
    {
        Ok(context) => context,
        Err(error) => {
            if let Some((space_uid, fence_id)) = state
                .identity
                .owner_recovery_abort_fence_for_challenge(payload.challenge_id)
                .await
                .map_err(|_| recovery_storage_unavailable())?
            {
                abort_owner_recovery_fence(&state, space_uid, fence_id).await?;
            }
            return Err(owner_recovery_api_error(error));
        }
    };
    validate_owner_recovery_context(&state, &context).await?;
    let result = state
        .identity
        .finish_owner_recovery_registration(payload.challenge_id, &payload.credential)
        .await
        .map_err(owner_recovery_api_error)?;
    let fence_id = context
        .recovery_fence_id
        .ok_or_else(recovery_fence_unavailable)?;
    let space_uid = context.space_uid;
    let space_id = find_space_id_by_uid(&state, space_uid).await.ok();
    let space_fence_committed = if let Some(space_id) = space_id.as_deref() {
        Authorizer::new(state.service.operator().clone())
            .complete_recovery_fence(space_id, fence_id)
            .await
            .is_ok()
    } else {
        false
    };
    let node_fence_committed = if space_fence_committed {
        state
            .identity
            .complete_recovery_fence(fence_id)
            .await
            .is_ok()
    } else {
        false
    };
    if !space_fence_committed || !node_fence_committed {
        // The committed reset is terminal, but its one-time response must not
        // be manufactured while the matching Space fence is pending.
        return Err(recovery_fence_unavailable());
    }
    if space_fence_committed && node_fence_committed {
        let _ = state
            .identity
            .mark_recovery_fence_reconciled(fence_id)
            .await;
    }
    let mut headers = recovery_result_headers();
    let cookie = auth_cookie(&result.session_id, 60 * 60 * 24 * 30);
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "invalid auth cookie"))?,
    );
    let recovery_event_id = result.recovery_request_id.unwrap_or(payload.challenge_id);
    let node_audit_status = state
        .identity
        .append_node_audit_with_id(
            recovery_event_id,
            NodeAuditInput {
            subject_account_id: Some(result.account.account_id),
            actor_account_id: result.recovery_issuer_account_id,
            credential_id: None,
            action: "recovery.owner_reset_completed",
            target_type: "human_account",
            target_id: Some(result.account.account_id.to_string()),
            outcome: "success",
            request_id: Some(recovery_event_id.to_string()),
            safe_metadata: json!({"credential_generation": result.account.credential_generation}),
            },
        )
        .await
        .is_ok();
    if node_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(recovery_event_id, "node")
            .await;
    }
    let space_audit_status = match (
        result.recovery_space_uid,
        result.recovery_principal_id,
        result.recovery_issuer_principal_id,
        result.recovery_request_id,
    ) {
        (Some(space_uid), Some(_principal_id), Some(_issuer_principal_id), Some(request_id)) => {
            match find_space_id_by_uid(&state, space_uid).await {
                Ok(space_id) => append_recovery_space_audit(&state, &space_id, request_id).await,
                Err(_) => false,
            }
        }
        _ => false,
    };
    if space_audit_status {
        let _ = state
            .identity
            .mark_recovery_audit_stage(recovery_event_id, "space")
            .await;
    }
    let audit_delivered = if node_audit_status && space_audit_status {
        state
            .identity
            .mark_recovery_audit_delivered(recovery_event_id)
            .await
            .is_ok()
    } else {
        false
    };
    Ok((
        StatusCode::CREATED,
        headers,
        Json(json!({
            "account": result.account,
            "recovery_codes": result.recovery_codes,
            "audit_status": if audit_delivered && space_fence_committed && node_fence_committed { "delivered" } else { "pending" }
        })),
    )
        .into_response())
}

fn owner_recovery_api_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    let lower = message.to_lowercase();
    if message.contains("RECOVERY_FENCE_UNAVAILABLE") {
        return recovery_fence_unavailable();
    }
    if lower.contains("compare-and-swap")
        || lower.contains("control-store")
        || lower.contains("control object")
        || lower.contains("node control write")
        || lower.contains("invalid stored timestamp")
        || lower.contains("storage")
        || lower.contains("failed to read")
        || lower.contains("failed to write")
    {
        return recovery_storage_unavailable();
    }
    let (status, code) = if message.contains("already pending") {
        (StatusCode::CONFLICT, "OWNER_RECOVERY_CHALLENGE_PENDING")
    } else if message.contains("already completed") {
        (StatusCode::CONFLICT, "OWNER_RESET_ALREADY_COMPLETED")
    } else if message.contains("expired") {
        (StatusCode::GONE, "OWNER_APPROVAL_EXPIRED")
    } else if message.contains("verify owner recovery") {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "WEBAUTHN_REGISTRATION_INVALID",
        )
    } else {
        (StatusCode::UNAUTHORIZED, "AUTHENTICATION_REQUIRED")
    };
    ApiError::new(
        StatusCode::from_u16(status.as_u16()).unwrap_or(status),
        json!({
            "code": code,
            "message": if code == "WEBAUTHN_REGISTRATION_INVALID" { "WebAuthn registration is invalid" } else { "owner recovery is invalid" }
        }),
    )
}

async fn auth_session(State(state): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let account = match auth_session_cookie(&headers) {
        Some(session_id) => state
            .identity
            .authenticate_session(&session_id)
            .await
            .ok()
            .map(|session| session.account),
        None => None,
    };
    Json(match account {
        Some(account) => json!({"authenticated": true, "account": account}),
        None => json!({"authenticated": false}),
    })
}

async fn auth_session_delete(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session_id) = auth_session_cookie(&headers) {
        let authenticated = state.identity.authenticate_session(&session_id).await.ok();
        let _ = state.identity.revoke_session(&session_id).await;
        if let Some(authenticated) = authenticated {
            let _ = state
                .identity
                .append_node_audit(NodeAuditInput {
                    subject_account_id: Some(authenticated.account.account_id),
                    actor_account_id: Some(authenticated.account.account_id),
                    credential_id: Some(authenticated.credential_id),
                    action: "session.revoked",
                    target_type: "browser_session",
                    target_id: Some(authenticated.session_id.to_string()),
                    outcome: "success",
                    request_id: None,
                    safe_metadata: json!({}),
                })
                .await;
        }
    }
    (
        StatusCode::OK,
        [("set-cookie", clear_auth_cookie())],
        Json(json!({"authenticated": false})),
    )
        .into_response()
}

async fn list_accounts(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "node admin role is required",
        ));
    }
    Ok(Json(
        serde_json::to_value(state.identity.list_accounts().await.map_err(auth_error)?)
            .map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct AccountStatusRequest {
    status: AccountStatus,
}

async fn set_account_status(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(account_id): Path<Uuid>,
    Json(payload): Json<AccountStatusRequest>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "node admin role is required",
        ));
    }
    for space_id in state
        .service
        .list_space_ids()
        .await
        .map_err(ApiError::from_core)?
    {
        let space_uid = state
            .service
            .space_uid(&space_id)
            .await
            .map_err(ApiError::from_core)?;
        let bound = state
            .identity
            .bindings_for_space(space_uid)
            .await
            .map_err(auth_error)?
            .into_iter()
            .any(|binding| binding.node_account_id == account_id);
        if bound
            && has_active_recovery_fence(
                &Authorizer::new(state.service.operator().clone())
                    .state(&space_id)
                    .await
                    .map_err(ApiError::from_core)?,
            )
        {
            return Err(recovery_fence_unavailable());
        }
    }
    let account = state
        .identity
        .set_account_status(account_id, payload.status)
        .await
        .map_err(recovery_aware_auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "node_account.status_changed",
            target_type: "human_account",
            target_id: Some(account_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"status": account.status}),
        })
        .await
        .map_err(auth_error)?;
    Ok(Json(
        serde_json::to_value(account).map_err(|error| auth_error(error.into()))?,
    ))
}

fn auth_session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "ugoite_session").then(|| value.to_string())
            })
        })
}

fn auth_cookie(token: &str, max_age_seconds: i64) -> String {
    let secure = env::var("UGOITE_PUBLIC_ORIGIN")
        .unwrap_or_default()
        .starts_with("https://");
    format!(
        "ugoite_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn clear_auth_cookie() -> String {
    "ugoite_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string()
}

fn auth_error(_error: anyhow::Error) -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        json!({
            "code": "AUTHENTICATION_FAILED",
            "message": "Authentication failed",
        }),
    )
}

fn recovery_aware_auth_error(error: anyhow::Error) -> ApiError {
    if error.to_string().contains("RECOVERY_FENCE_UNAVAILABLE") {
        return recovery_fence_unavailable();
    }
    auth_error(error)
}

#[derive(Deserialize)]
struct OidcProviderPayload {
    issuer: String,
    client_id: String,
    client_secret: Option<String>,
}

async fn configure_oidc_provider(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<OidcProviderPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_recent_passkey(&identity)?;
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "recent node-admin Passkey session is required",
        ));
    }
    // Discovery is performed before persistence so invalid issuers never become active.
    let http = oidc_http_client().map_err(auth_error)?;
    CoreProviderMetadata::discover_async(
        IssuerUrl::new(payload.issuer.clone()).map_err(|error| auth_error(error.into()))?,
        &http,
    )
    .await
    .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?;
    let provider = state
        .identity
        .configure_oidc_provider(
            identity.account_id,
            &payload.issuer,
            &payload.client_id,
            payload.client_secret,
        )
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "oidc_provider.configured",
            target_type: "oidc_provider",
            target_id: Some(provider.provider_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"issuer": provider.issuer}),
        })
        .await
        .map_err(auth_error)?;
    let mut value = serde_json::to_value(provider).map_err(|error| auth_error(error.into()))?;
    value["client_secret"] = Value::Null;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn list_oidc_providers(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .identity
                .list_oidc_providers()
                .await
                .map_err(auth_error)?,
        )
        .map_err(|error| auth_error(error.into()))?,
    ))
}

#[derive(Deserialize)]
struct OidcStartQuery {
    invitation_token: Option<String>,
}

async fn oidc_start(
    State(state): State<AppState>,
    Path(provider_id): Path<Uuid>,
    Query(query): Query<OidcStartQuery>,
) -> ApiResult<Redirect> {
    start_oidc_authorization(&state, provider_id, query.invitation_token.as_deref(), None).await
}

async fn oidc_link_start(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(provider_id): Path<Uuid>,
) -> ApiResult<Redirect> {
    require_recent_passkey(&identity)?;
    start_oidc_authorization(&state, provider_id, None, Some(identity.account_id)).await
}

async fn start_oidc_authorization(
    state: &AppState,
    provider_id: Uuid,
    invitation_token: Option<&str>,
    link_account_id: Option<Uuid>,
) -> ApiResult<Redirect> {
    let provider = state
        .identity
        .oidc_provider(provider_id)
        .await
        .map_err(auth_error)?;
    let http = oidc_http_client().map_err(auth_error)?;
    let metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(provider.issuer.clone()).map_err(|error| auth_error(error.into()))?,
        &http,
    )
    .await
    .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?;
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(provider.client_id),
        provider.client_secret.map(ClientSecret::new),
    )
    .set_redirect_uri(
        RedirectUrl::new(format!("{}/auth/oidc/callback", api_base_url(&issuer)))
            .map_err(|error| auth_error(error.into()))?,
    );
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();
    state
        .identity
        .save_oidc_attempt(
            provider_id,
            csrf.secret(),
            nonce.secret(),
            pkce_verifier.secret(),
            invitation_token,
            link_account_id,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok(Redirect::temporary(auth_url.as_str()))
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn oidc_callback(
    State(state): State<AppState>,
    Query(query): Query<OidcCallbackQuery>,
) -> ApiResult<Response> {
    let attempt = state
        .identity
        .consume_oidc_attempt(&query.state)
        .await
        .map_err(recovery_aware_auth_error)?;
    let provider = state
        .identity
        .oidc_provider(attempt.provider_id)
        .await
        .map_err(auth_error)?;
    let http = oidc_http_client().map_err(auth_error)?;
    let metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(provider.issuer.clone()).map_err(|error| auth_error(error.into()))?,
        &http,
    )
    .await
    .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?;
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(provider.client_id),
        provider.client_secret.map(ClientSecret::new),
    )
    .set_redirect_uri(
        RedirectUrl::new(format!("{}/auth/oidc/callback", api_base_url(&issuer)))
            .map_err(|error| auth_error(error.into()))?,
    );
    let token = client
        .exchange_code(AuthorizationCode::new(query.code))
        .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?
        .set_pkce_verifier(PkceCodeVerifier::new(attempt.pkce_verifier))
        .request_async(&http)
        .await
        .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?;
    let id_token = token
        .id_token()
        .ok_or_else(|| auth_error(anyhow::anyhow!("OIDC provider omitted id_token")))?;
    let claims = id_token
        .claims(&client.id_token_verifier(), &Nonce::new(attempt.nonce))
        .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))?;
    let subject = claims.subject().as_str();
    let linked_existing_account = attempt.link_account_id.is_some();
    let (account, session_id, invitation) = state
        .identity
        .complete_oidc_login(
            &provider.issuer,
            subject,
            subject,
            attempt.invitation_hash.as_deref(),
            attempt.link_account_id,
            attempt.link_account_generation,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    if let Some(invitation) = invitation {
        bind_invited_account(&state, &account, &invitation, BindingMethod::Oidc).await?;
        state
            .identity
            .complete_invitation_acceptance(
                invitation.invitation_id,
                account.account_id,
                invitation.accepted_principal_id().ok_or_else(|| {
                    ApiError::new(StatusCode::CONFLICT, "invitation acceptance is incomplete")
                })?,
            )
            .await
            .map_err(auth_error)?;
    }
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(account.account_id),
            actor_account_id: Some(account.account_id),
            credential_id: None,
            action: if linked_existing_account {
                "oidc.identity_linked"
            } else {
                "oidc.login"
            },
            target_type: "human_account",
            target_id: Some(account.account_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"issuer": provider.issuer}),
        })
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::SEE_OTHER,
        [
            ("set-cookie", auth_cookie(&session_id, 60 * 60 * 24 * 30)),
            (
                "location",
                if linked_existing_account {
                    "/settings/security"
                } else {
                    "/spaces"
                }
                .to_string(),
            ),
        ],
    )
        .into_response())
}

fn oidc_http_client() -> anyhow::Result<openidconnect::reqwest::Client> {
    Ok(openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()?)
}

async fn find_space_id_by_uid(state: &AppState, space_uid: Uuid) -> ApiResult<String> {
    for space_id in state
        .service
        .list_space_ids()
        .await
        .map_err(ApiError::from_core)?
    {
        if state
            .service
            .space_uid(&space_id)
            .await
            .map_err(ApiError::from_core)?
            == space_uid
        {
            return Ok(space_id);
        }
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "invited Space is not present on this node",
    ))
}

async fn bind_invited_account(
    state: &AppState,
    account: &HumanAccount,
    invitation: &AccountInvitation,
    binding_method: BindingMethod,
) -> ApiResult<()> {
    let Some(space_uid) = invitation.space_uid else {
        return Ok(());
    };
    let space_id = find_space_id_by_uid(state, space_uid).await?;
    reconcile_recovery_fences(state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let principal_id = invitation.accepted_principal_id().ok_or_else(|| {
        ApiError::new(StatusCode::CONFLICT, "invitation acceptance is incomplete")
    })?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let authorization = authorizer
        .state(&space_id)
        .await
        .map_err(recovery_aware_auth_error)?;
    if has_active_recovery_fence(&authorization) {
        return Err(recovery_fence_unavailable());
    }
    let active_space_member =
        authorization
            .principals
            .get(&principal_id)
            .is_some_and(|principal| {
                matches!(principal.kind, PrincipalKind::Human)
                    && matches!(principal.state, PrincipalState::Active)
                    && authorization.memberships.contains_key(&principal_id)
            });
    let principal_has_conflicting_space_state = authorization
        .principals
        .get(&principal_id)
        .is_some_and(|principal| {
            !active_space_member || !matches!(principal.kind, PrincipalKind::Human)
        });
    match state
        .identity
        .binding_for_account(space_uid, account.account_id)
        .await
        .map_err(auth_error)?
    {
        Some(existing_principal_id) if existing_principal_id == principal_id => {
            if active_space_member {
                return Ok(());
            }
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                json!({
                    "code": "SPACE_MEMBERSHIP_CONFLICT",
                    "message": "Node binding exists but the Space has no active membership for this principal",
                }),
            ));
        }
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                json!({
                    "code": "ACCOUNT_ALREADY_BOUND",
                    "message": "account is already bound to this Space",
                }),
            ));
        }
        None => {}
    }
    if principal_has_conflicting_space_state {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            json!({
                "code": "SPACE_MEMBERSHIP_CONFLICT",
                "message": "Space authorization state conflicts with the invitation principal",
            }),
        ));
    }
    if !active_space_member {
        let inviter = state
            .identity
            .principal_for_account(space_uid, invitation.created_by)
            .await
            .map_err(auth_error)?;
        authorizer
            .add_human_member(
                &space_id,
                inviter,
                SpacePrincipal {
                    principal_id,
                    kind: PrincipalKind::Human,
                    display_name: account.display_name.clone(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                parse_space_role(invitation.role.as_deref().unwrap_or("viewer"))?,
            )
            .await
            .map_err(recovery_aware_auth_error)?;
    }
    if has_active_recovery_fence(
        &authorizer
            .state(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    ) {
        return Err(recovery_fence_unavailable());
    }
    state
        .identity
        .add_binding(ugoite_domain::identity::PrincipalBinding {
            space_uid,
            principal_id,
            node_account_id: account.account_id,
            binding_method,
        })
        .await
        .map_err(recovery_aware_auth_error)
}

fn parse_space_role(role: &str) -> ApiResult<SpaceRole> {
    match role {
        "owner" => Ok(SpaceRole::Owner),
        "editor" => Ok(SpaceRole::Editor),
        "viewer" => Ok(SpaceRole::Viewer),
        _ => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid Space role",
        )),
    }
}

async fn oauth_protected_resource_metadata(
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    Ok(Json(json!({
        "resource": issuer,
        "authorization_servers": [issuer],
        "scopes_supported": ["read", "create", "update", "delete", "share"],
        "bearer_methods_supported": [],
        "resource_documentation": OAUTH_RESOURCE_DOCUMENTATION_URL
    })))
}

fn api_base_url(issuer: &str) -> String {
    env::var("UGOITE_API_BASE_URL")
        .unwrap_or_else(|_| issuer.to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn oauth_authorization_server_metadata(
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    Ok(Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{}/oauth/authorize", api_base_url(&issuer)),
        "device_authorization_endpoint": format!("{}/oauth/device/authorization", api_base_url(&issuer)),
        "token_endpoint": format!("{}/oauth/token", api_base_url(&issuer)),
        "revocation_endpoint": format!("{}/oauth/revoke", api_base_url(&issuer)),
        "grant_types_supported": ["authorization_code", "urn:ietf:params:oauth:grant-type:device_code", "refresh_token", "client_credentials"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["private_key_jwt"],
        "dpop_signing_alg_values_supported": ["ES256"],
        "scopes_supported": ["read", "create", "update"]
    })))
}

#[derive(Clone, Deserialize)]
struct AuthorizePayload {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    state: String,
    space_id: String,
    scope: String,
    public_key_jwk: String,
}

fn validate_authorize_payload(payload: &AuthorizePayload) -> ApiResult<(Value, BTreeSet<String>)> {
    if payload.response_type != "code"
        || payload.code_challenge_method != "S256"
        || payload.code_challenge.len() < 43
        || payload.state.trim().is_empty()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid authorization request",
        ));
    }
    let redirect = url::Url::parse(&payload.redirect_uri)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid redirect_uri"))?;
    let host = redirect.host_str().unwrap_or_default();
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if redirect.fragment().is_some() || (redirect.scheme() != "https" && !loopback) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "redirect_uri must use HTTPS or a loopback host",
        ));
    }
    let public_key_jwk: Value = serde_json::from_str(&payload.public_key_jwk)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid public_key_jwk"))?;
    oauth::jwk_thumbprint(&public_key_jwk).map_err(auth_error)?;
    let actions = payload
        .scope
        .split_ascii_whitespace()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    validate_action_names(&actions)?;
    validate_access_credential_actions(&actions)?;
    Ok((public_key_jwk, actions))
}

async fn oauth_authorize(
    Extension(identity): Extension<RequestIdentityContext>,
    Query(payload): Query<AuthorizePayload>,
) -> ApiResult<Html<String>> {
    require_recent_passkey(&identity)?;
    let (_, actions) = validate_authorize_payload(&payload)?;
    let hidden = |name: &str, value: &str| {
        format!(
            "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
            html_escape(name),
            html_escape(value)
        )
    };
    let mut fields = String::new();
    for (name, value) in [
        ("response_type", payload.response_type.as_str()),
        ("client_id", payload.client_id.as_str()),
        ("redirect_uri", payload.redirect_uri.as_str()),
        ("code_challenge", payload.code_challenge.as_str()),
        (
            "code_challenge_method",
            payload.code_challenge_method.as_str(),
        ),
        ("state", payload.state.as_str()),
        ("space_id", payload.space_id.as_str()),
        ("scope", payload.scope.as_str()),
        ("public_key_jwk", payload.public_key_jwk.as_str()),
    ] {
        fields.push_str(&hidden(name, value));
    }
    Ok(Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Authorize Ugoite client</title><main><h1>Authorize client</h1><dl><dt>Client</dt><dd>{}</dd><dt>Space</dt><dd>{}</dd><dt>Actions</dt><dd>{}</dd></dl><form method=\"post\">{}<button type=\"submit\">Approve</button></form></main>",
        html_escape(&payload.client_id),
        html_escape(&payload.space_id),
        html_escape(&actions.into_iter().collect::<Vec<_>>().join(", ")),
        fields
    )))
}

async fn oauth_authorize_approve(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Form(payload): Form<AuthorizePayload>,
) -> ApiResult<Redirect> {
    require_recent_passkey(&identity)?;
    let (public_key_jwk, actions) = validate_authorize_payload(&payload)?;
    let principal_id = principal_for_space(&state, &payload.space_id, &identity).await?;
    let effective = Authorizer::new(state.service.operator().clone())
        .effective_actions(&payload.space_id, principal_id, None)
        .await
        .map_err(ApiError::from_core)?;
    if actions
        .iter()
        .any(|action| parse_action(action).is_ok_and(|action| !effective.contains(&action)))
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "requested scope exceeds the principal's permission",
        ));
    }
    let space_uid = state
        .service
        .space_uid(&payload.space_id)
        .await
        .map_err(ApiError::from_core)?;
    let code = state
        .identity
        .issue_authorization_code(
            &payload.client_id,
            &payload.redirect_uri,
            &payload.code_challenge,
            public_key_jwk,
            identity.account_id,
            principal_id,
            space_uid,
            actions,
        )
        .await
        .map_err(auth_error)?;
    let mut redirect = url::Url::parse(&payload.redirect_uri)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid redirect_uri"))?;
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &payload.state);
    Ok(Redirect::to(redirect.as_str()))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Deserialize)]
struct RevokeTokenPayload {
    token: String,
    credential_id: Uuid,
    client_assertion: String,
}

async fn oauth_revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> ApiResult<StatusCode> {
    let payload: RevokeTokenPayload = decode_oauth_payload(&headers, request).await?;
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let audience = format!("{}/oauth/revoke", api_base_url(&issuer));
    let public_key = state
        .identity
        .oauth_credential_public_key(payload.credential_id)
        .await
        .map_err(auth_error)?;
    let assertion =
        oauth::verify_client_assertion(&payload.client_assertion, &public_key, &audience)
            .map_err(auth_error)?;
    state
        .identity
        .record_proof_jti(&assertion.jti)
        .await
        .map_err(auth_error)?;
    state
        .identity
        .revoke_oauth_token(&payload.token, payload.credential_id)
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct DeviceAuthorizationPayload {
    device_name: String,
    public_key_jwk: Value,
    space_uid: Option<Uuid>,
    #[serde(default)]
    requested_actions: BTreeSet<String>,
}

async fn oauth_device_authorization(
    State(state): State<AppState>,
    Json(mut payload): Json<DeviceAuthorizationPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    oauth::jwk_thumbprint(&payload.public_key_jwk).map_err(auth_error)?;
    if payload.requested_actions.is_empty() {
        payload.requested_actions.insert("read".to_string());
    }
    validate_action_names(&payload.requested_actions)?;
    validate_access_credential_actions(&payload.requested_actions)?;
    let response = state
        .identity
        .start_device_authorization(
            &payload.device_name,
            payload.public_key_jwk,
            payload.space_uid,
            payload.requested_actions,
        )
        .await
        .map_err(auth_error)?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[derive(Deserialize)]
struct DeviceApprovePayload {
    user_code: String,
    space_id: String,
    #[serde(default)]
    granted_actions: BTreeSet<String>,
}

#[derive(Deserialize)]
struct DevicePendingQuery {
    user_code: String,
}

async fn oauth_device_pending(
    State(state): State<AppState>,
    Query(query): Query<DevicePendingQuery>,
) -> ApiResult<Json<Value>> {
    let pending = state
        .identity
        .pending_device_authorization(&query.user_code)
        .await
        .map_err(auth_error)?;
    Ok(Json(json!({
        "device_name": pending.device_name,
        "requested_space_uid": pending.requested_space_uid,
        "requested_actions": pending.requested_actions,
        "expires_at": pending.expires_at,
    })))
}

async fn oauth_device_approve(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(mut payload): Json<DeviceApprovePayload>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    let pending = state
        .identity
        .pending_device_authorization(&payload.user_code)
        .await
        .map_err(auth_error)?;
    let space_uid = state
        .service
        .space_uid(&payload.space_id)
        .await
        .map_err(ApiError::from_core)?;
    if pending
        .requested_space_uid
        .is_some_and(|requested| requested != space_uid)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "approved Space differs from the CLI request",
        ));
    }
    let principal_id = principal_for_space(&state, &payload.space_id, &identity).await?;
    if payload.granted_actions.is_empty() {
        payload.granted_actions = pending.requested_actions.clone();
    }
    validate_action_names(&payload.granted_actions)?;
    validate_access_credential_actions(&payload.granted_actions)?;
    if !payload
        .granted_actions
        .is_subset(&pending.requested_actions)
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "approval cannot expand requested actions",
        ));
    }
    let effective = Authorizer::new(state.service.operator().clone())
        .effective_actions(&payload.space_id, principal_id, None)
        .await
        .map_err(ApiError::from_core)?;
    for action in &payload.granted_actions {
        let required = parse_action(action)?;
        if !effective.contains(&required) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                format!("principal cannot grant {action}"),
            ));
        }
    }
    state
        .identity
        .approve_device_authorization(
            &payload.user_code,
            identity.account_id,
            principal_id,
            space_uid,
            payload.granted_actions.clone(),
        )
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "oauth_grant.approved",
            target_type: "cli_device_request",
            target_id: None,
            outcome: "success",
            request_id: None,
            safe_metadata: json!({
                "space_uid": space_uid,
                "device_name": pending.device_name,
                "granted_actions": payload.granted_actions,
            }),
        })
        .await
        .map_err(auth_error)?;
    Ok(Json(
        json!({"approved": true, "space_uid": space_uid, "granted_actions": payload.granted_actions}),
    ))
}

#[derive(Deserialize)]
struct TokenPayload {
    grant_type: String,
    device_code: Option<String>,
    refresh_token: Option<String>,
    code: Option<String>,
    code_verifier: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    client_assertion: Option<String>,
}

async fn oauth_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> ApiResult<Json<Value>> {
    let payload: TokenPayload = decode_oauth_payload(&headers, request).await?;
    let (issuer, node_id) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let audience = format!("{}/oauth/token", api_base_url(&issuer));
    let (credential, refresh, refresh_token, context) =
        if payload.grant_type == "authorization_code" {
            let code = payload
                .code
                .as_deref()
                .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "code is required"))?;
            let grant = state
                .identity
                .pending_authorization_code(code)
                .await
                .map_err(auth_error)?;
            let assertion = oauth::verify_client_assertion(
                payload.client_assertion.as_deref().ok_or_else(|| {
                    ApiError::new(StatusCode::BAD_REQUEST, "client_assertion is required")
                })?,
                &grant.public_key_jwk,
                &audience,
            )
            .map_err(auth_error)?;
            state
                .identity
                .record_proof_jti(&assertion.jti)
                .await
                .map_err(auth_error)?;
            state
                .identity
                .exchange_authorization_code(
                    code,
                    payload.client_id.as_deref().ok_or_else(|| {
                        ApiError::new(StatusCode::BAD_REQUEST, "client_id is required")
                    })?,
                    payload.redirect_uri.as_deref().ok_or_else(|| {
                        ApiError::new(StatusCode::BAD_REQUEST, "redirect_uri is required")
                    })?,
                    payload.code_verifier.as_deref().ok_or_else(|| {
                        ApiError::new(StatusCode::BAD_REQUEST, "code_verifier is required")
                    })?,
                )
                .await
                .map_err(auth_error)?
        } else if payload.grant_type == "urn:ietf:params:oauth:grant-type:device_code" {
            let device_code = payload
                .device_code
                .as_deref()
                .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "device_code is required"))?;
            let pending = state
                .identity
                .pending_device_by_device_code(device_code)
                .await
                .map_err(|error| match error.to_string().as_str() {
                    "slow_down" => {
                        ApiError::new(StatusCode::BAD_REQUEST, json!({"error": "slow_down"}))
                    }
                    message if message.contains("expired") => {
                        ApiError::new(StatusCode::BAD_REQUEST, json!({"error": "expired_token"}))
                    }
                    _ => auth_error(error),
                })?;
            let assertion = oauth::verify_client_assertion(
                payload.client_assertion.as_deref().ok_or_else(|| {
                    ApiError::new(StatusCode::BAD_REQUEST, "client_assertion is required")
                })?,
                &pending.public_key_jwk,
                &audience,
            )
            .map_err(auth_error)?;
            state
                .identity
                .record_proof_jti(&assertion.jti)
                .await
                .map_err(auth_error)?;
            state
                .identity
                .exchange_device_code(device_code)
                .await
                .map_err(|error| {
                    if error.to_string() == "authorization_pending" {
                        ApiError::new(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "authorization_pending"}),
                        )
                    } else {
                        auth_error(error)
                    }
                })?
        } else if payload.grant_type == "refresh_token" {
            let old_token = payload.refresh_token.as_deref().ok_or_else(|| {
                ApiError::new(StatusCode::BAD_REQUEST, "refresh_token is required")
            })?;
            let old = state
                .identity
                .refresh_credential(old_token)
                .await
                .map_err(auth_error)?;
            let credential = state
                .identity
                .device_credential(old.credential_id)
                .await
                .map_err(auth_error)?;
            let assertion = oauth::verify_client_assertion(
                payload.client_assertion.as_deref().ok_or_else(|| {
                    ApiError::new(StatusCode::BAD_REQUEST, "client_assertion is required")
                })?,
                &credential.public_key_jwk,
                &audience,
            )
            .map_err(auth_error)?;
            state
                .identity
                .record_proof_jti(&assertion.jti)
                .await
                .map_err(auth_error)?;
            let (new_token, rotated, context) = state
                .identity
                .rotate_refresh_credential(old_token)
                .await
                .map_err(auth_error)?;
            (credential, rotated, new_token, context)
        } else {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "unsupported grant_type",
            ));
        };
    let now = chrono::Utc::now().timestamp();
    let thumbprint = oauth::jwk_thumbprint(&credential.public_key_jwk).map_err(auth_error)?;
    let claims = AccessTokenClaims {
        iss: context["issuer"].as_str().unwrap_or(&issuer).to_string(),
        node_id,
        sub: refresh.principal_id,
        principal_type: "human".to_string(),
        actor_principal_id: None,
        aud: issuer,
        space_uid: refresh.space_uid,
        granted_actions: refresh.granted_actions,
        actor_chain: vec![refresh.principal_id],
        exp: now + 300,
        iat: now,
        jti: Uuid::now_v7(),
        credential_id: credential.credential_id,
        credential_generation: Some(refresh.credential_generation),
        cnf: Confirmation { jkt: thumbprint },
    };
    let access_token = state
        .identity
        .issue_access_credential(claims.clone())
        .await
        .map_err(auth_error)?;
    Ok(Json(json!({
        "access_token": access_token,
        "token_type": "DPoP",
        "expires_in": 300,
        "refresh_token": refresh_token,
        "scope": claims.granted_actions.into_iter().collect::<Vec<_>>().join(" "),
        "credential_id": credential.credential_id,
        "space_uid": claims.space_uid
    })))
}

async fn decode_oauth_payload<T: serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    request: Request,
) -> ApiResult<T> {
    let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid OAuth request body"))?;
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"))
    {
        serde_urlencoded::from_bytes(&bytes)
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid OAuth form body"))
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid OAuth JSON body"))
    }
}

async fn list_device_credentials(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .identity
                .list_device_credentials(identity.account_id)
                .await
                .map_err(auth_error)?,
        )
        .map_err(|error| auth_error(error.into()))?,
    ))
}

async fn revoke_device_credential(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(credential_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .identity
        .revoke_device_credential(identity.account_id, credential_id)
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(credential_id),
            action: "device.revoked",
            target_type: "cli_device",
            target_id: Some(credential_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AgentCreatePayload {
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    mode: AgentMode,
    public_key_jwk: Value,
    #[serde(default)]
    owner_principal_ids: BTreeSet<Uuid>,
    #[serde(default)]
    granted_actions: BTreeSet<String>,
    expires_at: Option<String>,
}

async fn create_agent(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(mut payload): Json<AgentCreatePayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_recent_passkey(&identity)?;
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    oauth::jwk_thumbprint(&payload.public_key_jwk).map_err(auth_error)?;
    let sponsor = principal_for_space(&state, &space_id, &identity).await?;
    if payload.owner_principal_ids.is_empty() {
        payload.owner_principal_ids.insert(sponsor);
    }
    if payload.granted_actions.is_empty() {
        payload.granted_actions.insert("read".to_string());
    }
    validate_action_names(&payload.granted_actions)?;
    let grants = payload
        .granted_actions
        .iter()
        .map(|action| parse_action(action))
        .collect::<ApiResult<BTreeSet<_>>>()?;
    let agent = Authorizer::new(state.service.operator().clone())
        .create_agent(
            &space_id,
            sponsor,
            ugoite_iceberg::authorization::CreateAgentRequest {
                display_name: payload.display_name,
                description: payload.description,
                mode: payload.mode,
                owner_principal_ids: payload.owner_principal_ids,
                granted_actions: grants,
                expires_at: payload.expires_at.clone(),
            },
        )
        .await
        .map_err(ApiError::from_core)?;
    let credential = state
        .identity
        .register_agent_credential(agent.agent_id, payload.public_key_jwk, payload.expires_at)
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(credential.credential_id),
            action: "agent_credential.registered",
            target_type: "agent",
            target_id: Some(agent.agent_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"space_id": space_id}),
        })
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"agent": agent, "credential": credential})),
    ))
}

async fn list_agents(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    // Reading the agent inventory is navigation, not a privileged mutation.
    // Keep the recent-passkey requirement on create/revoke/token issuance, but
    // do not make opening the settings page spuriously require a fresh passkey.
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        serde_json::to_value(authorization.agents.into_values().collect::<Vec<_>>())
            .map_err(|error| auth_error(error.into()))?,
    ))
}

async fn revoke_agent(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, agent_id)): Path<(String, Uuid)>,
) -> ApiResult<StatusCode> {
    require_recent_passkey(&identity)?;
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let actor = principal_for_space(&state, &space_id, &identity).await?;
    Authorizer::new(state.service.operator().clone())
        .revoke_agent(&space_id, actor, agent_id)
        .await
        .map_err(ApiError::from_core)?;
    state
        .identity
        .revoke_agent_credentials(agent_id)
        .await
        .map_err(auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: None,
            action: "agent.revoked",
            target_type: "agent",
            target_id: Some(agent_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"space_id": space_id}),
        })
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AgentTokenPayload {
    credential_id: Uuid,
    client_assertion: String,
    space_id: String,
    #[serde(default)]
    requested_actions: BTreeSet<String>,
}

async fn issue_autonomous_agent_token(
    State(state): State<AppState>,
    Json(payload): Json<AgentTokenPayload>,
) -> ApiResult<Json<Value>> {
    let credential = state
        .identity
        .agent_credential(payload.credential_id)
        .await
        .map_err(auth_error)?;
    let agent = Authorizer::new(state.service.operator().clone())
        .state(&payload.space_id)
        .await
        .map_err(ApiError::from_core)?
        .agents
        .get(&credential.agent_id)
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "agent is not active in this Space"))?;
    if !agent.mode.allows_autonomous() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "agent is not enabled for autonomous operation",
        ));
    }
    let (issuer, node_id) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let audience = format!("{}/oauth/agent/token", api_base_url(&issuer));
    let assertion = oauth::verify_client_assertion(
        &payload.client_assertion,
        &credential.public_key_jwk,
        &audience,
    )
    .map_err(auth_error)?;
    state
        .identity
        .record_proof_jti(&assertion.jti)
        .await
        .map_err(auth_error)?;
    issue_agent_token(
        &state,
        &payload.space_id,
        credential.agent_id,
        credential.credential_id,
        &credential.public_key_jwk,
        payload.requested_actions,
        None,
        &issuer,
        node_id,
    )
    .await
}

async fn issue_delegated_agent_token(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, agent_id)): Path<(String, Uuid)>,
    Json(payload): Json<AgentTokenPayload>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    if payload.space_id != space_id {
        return Err(ApiError::new(StatusCode::CONFLICT, "Space mismatch"));
    }
    let credential = state
        .identity
        .agent_credential(payload.credential_id)
        .await
        .map_err(auth_error)?;
    if credential.agent_id != agent_id {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "credential belongs to another agent",
        ));
    }
    let agent = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?
        .agents
        .get(&agent_id)
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "agent is not active in this Space"))?;
    if !agent.mode.allows_delegated() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "agent is not enabled for delegated operation",
        ));
    }
    let (issuer, node_id) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let audience = format!("{}/oauth/agent/token", api_base_url(&issuer));
    let assertion = oauth::verify_client_assertion(
        &payload.client_assertion,
        &credential.public_key_jwk,
        &audience,
    )
    .map_err(auth_error)?;
    state
        .identity
        .record_proof_jti(&assertion.jti)
        .await
        .map_err(auth_error)?;
    let human = principal_for_space(&state, &space_id, &identity).await?;
    issue_agent_token(
        &state,
        &space_id,
        agent_id,
        credential.credential_id,
        &credential.public_key_jwk,
        payload.requested_actions,
        Some(human),
        &issuer,
        node_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn issue_agent_token(
    state: &AppState,
    space_id: &str,
    agent_id: Uuid,
    credential_id: Uuid,
    public_key_jwk: &Value,
    mut requested_actions: BTreeSet<String>,
    on_behalf_of: Option<Uuid>,
    issuer: &str,
    node_id: Uuid,
) -> ApiResult<Json<Value>> {
    validate_action_names(&requested_actions)?;
    validate_access_credential_actions(&requested_actions)?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let agent_actions = authorizer
        .effective_actions(space_id, agent_id, None)
        .await
        .map_err(ApiError::from_core)?;
    let effective = if let Some(human) = on_behalf_of {
        let human_actions = authorizer
            .effective_actions(space_id, human, None)
            .await
            .map_err(ApiError::from_core)?;
        delegated_agent_actions(&agent_actions, &human_actions)
    } else {
        agent_actions
    };
    if requested_actions.is_empty() {
        requested_actions = effective
            .iter()
            .map(|action| action_name(action).to_string())
            .collect();
    }
    for action in &requested_actions {
        if !effective.contains(&parse_action(action)?) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                format!("agent cannot perform {action}"),
            ));
        }
    }
    let space_uid = state
        .service
        .space_uid(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let now = chrono::Utc::now().timestamp();
    let mut actor_chain = vec![agent_id];
    if let Some(human) = on_behalf_of {
        actor_chain.push(human);
    }
    let claims = AccessTokenClaims {
        iss: issuer.to_string(),
        node_id,
        sub: on_behalf_of.unwrap_or(agent_id),
        principal_type: if on_behalf_of.is_some() {
            "human".to_string()
        } else {
            "agent".to_string()
        },
        actor_principal_id: Some(agent_id),
        aud: issuer.to_string(),
        space_uid,
        granted_actions: requested_actions,
        actor_chain,
        exp: now + 300,
        iat: now,
        jti: Uuid::now_v7(),
        credential_id,
        credential_generation: None,
        cnf: Confirmation {
            jkt: oauth::jwk_thumbprint(public_key_jwk).map_err(auth_error)?,
        },
    };
    let access_token = state
        .identity
        .issue_access_credential(claims.clone())
        .await
        .map_err(auth_error)?;
    state
        .identity
        .mark_agent_credential_used(credential_id)
        .await
        .map_err(auth_error)?;
    authorizer
        .mark_agent_used(space_id, agent_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        json!({"access_token": access_token, "token_type": "DPoP", "expires_in": 300, "space_uid": space_uid, "actor_chain": claims.actor_chain}),
    ))
}

fn validate_action_names(actions: &BTreeSet<String>) -> ApiResult<()> {
    for action in actions {
        parse_action(action)?;
    }
    Ok(())
}

fn access_token_agent_id(claims: &AccessTokenClaims) -> &Uuid {
    claims.actor_principal_id.as_ref().unwrap_or(&claims.sub)
}

fn delegated_agent_actions(
    agent_actions: &BTreeSet<Action>,
    human_actions: &BTreeSet<Action>,
) -> BTreeSet<Action> {
    agent_actions.intersection(human_actions).cloned().collect()
}

fn validate_access_credential_actions(actions: &BTreeSet<String>) -> ApiResult<()> {
    if actions.contains("delete") || actions.contains("share") {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "delete and share are unavailable to CLI and agent credentials until interactive approval is implemented",
        ));
    }
    Ok(())
}

fn parse_action(action: &str) -> ApiResult<Action> {
    match action {
        "read" => Ok(Action::Read),
        "create" => Ok(Action::Create),
        "update" => Ok(Action::Update),
        "delete" => Ok(Action::Delete),
        "share" => Ok(Action::Share),
        _ => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unsupported action: {action}"),
        )),
    }
}

fn validate_id(value: &str, name: &str) -> ApiResult<()> {
    let kind = match name {
        "space_id" => IdentifierKind::Space,
        "entry_id" => IdentifierKind::Entry,
        "form_name" => IdentifierKind::Form,
        "asset_id" => IdentifierKind::Asset,
        "sql_id" => IdentifierKind::Sql,
        "session_id" | "sql_session_id" => IdentifierKind::SqlSession,
        "revision_id" => IdentifierKind::Revision,
        _ => IdentifierKind::Entry,
    };
    validate_identifier(kind, value).map_err(|error| ApiError::invalid_identifier(kind, error))
}

async fn require_space_permission(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    permission: SpacePermission,
) -> ApiResult<()> {
    let action = match permission {
        SpacePermission::Read => Action::Read,
        SpacePermission::WriteContent => Action::Update,
        SpacePermission::ManageSpace | SpacePermission::ManageMembers => Action::Share,
    };
    require_space_action(state, space_id, identity, action)
        .await
        .map(|_| ())
}

async fn require_space_action(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
) -> ApiResult<Uuid> {
    validate_id(space_id, "space_id")?;
    if matches!(action, Action::Delete | Action::Share) {
        require_recent_passkey(identity)?;
    }
    let principal_id = principal_for_space(state, space_id, identity).await?;
    if let Some(actions) = &identity.token_actions {
        let required = action_name(&action);
        if !actions.contains(required) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token does not grant the required action",
            ));
        }
    }
    Authorizer::new(state.service.operator().clone())
        .require(space_id, principal_id, action.clone(), None)
        .await
        .map_err(ApiError::from_core)?;
    if let Some(actor_principal_id) = identity.token_actor_principal_id {
        if actor_principal_id != principal_id {
            Authorizer::new(state.service.operator().clone())
                .require(space_id, actor_principal_id, action, None)
                .await
                .map_err(ApiError::from_core)?;
        }
    }
    Ok(principal_id)
}

async fn principal_for_space(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
) -> ApiResult<Uuid> {
    let space_uid = state
        .service
        .space_uid(space_id)
        .await
        .map_err(ApiError::from_core)?;
    if let Some(token_space_uid) = identity.token_space_uid {
        if token_space_uid != space_uid {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token audience is a different Space",
            ));
        }
        return identity
            .token_principal_id
            .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "token principal is missing"));
    }
    state
        .identity
        .principal_for_account(space_uid, identity.account_id)
        .await
        .map_err(auth_error)
}

fn authorization_principal_ids(
    identity: &RequestIdentityContext,
    subject_principal_id: Uuid,
) -> Vec<Uuid> {
    let mut principals = vec![subject_principal_id];
    if let Some(actor_principal_id) = identity.token_actor_principal_id {
        if actor_principal_id != subject_principal_id {
            principals.push(actor_principal_id);
        }
    }
    principals
}

async fn require_resource_action(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
    kind: ResourceKind,
    resource_id: &str,
) -> ApiResult<Uuid> {
    if matches!(action, Action::Delete | Action::Share) {
        require_recent_passkey(identity)?;
    }
    if let Some(actions) = &identity.token_actions {
        if !actions.contains(action_name(&action)) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token does not grant the required action",
            ));
        }
    }
    let principal_id = principal_for_space(state, space_id, identity).await?;
    state
        .service
        .require_resource_action(
            space_id,
            principal_id,
            action.clone(),
            kind.clone(),
            resource_id,
            None,
        )
        .await
        .map_err(ApiError::from_core)?;
    if let Some(actor_principal_id) = identity.token_actor_principal_id {
        if actor_principal_id != principal_id {
            state
                .service
                .require_resource_action(
                    space_id,
                    actor_principal_id,
                    action,
                    kind,
                    resource_id,
                    None,
                )
                .await
                .map_err(ApiError::from_core)?;
        }
    }
    Ok(principal_id)
}

fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Read => "read",
        Action::Create => "create",
        Action::Update => "update",
        Action::Delete => "delete",
        Action::Share => "share",
    }
}

fn parse_resource_kind(kind: &str) -> ApiResult<ResourceKind> {
    match kind {
        "entry" => Ok(ResourceKind::Entry),
        "asset" => Ok(ResourceKind::Asset),
        "form" => Ok(ResourceKind::Form),
        "saved_sql" => Ok(ResourceKind::SavedSql),
        "materialized_view" => Ok(ResourceKind::MaterializedView),
        _ => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported policy resource kind",
        )),
    }
}

async fn get_access_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, kind, resource_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    let resource_kind = parse_resource_kind(&kind)?;
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        resource_kind.clone(),
        &resource_id,
    )
    .await?;
    let resource = ugoite_iceberg::authorization::ResourceRef {
        kind: resource_kind,
        id: resource_id,
        parent: None,
    };
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        serde_json::to_value(authorization.policies.get(&resource.key()))
            .map_err(|error| auth_error(error.into()))?,
    ))
}

async fn put_access_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, kind, resource_id)): Path<(String, String, String)>,
    Json(policy): Json<AccessPolicy>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    let actor = principal_for_space(&state, &space_id, &identity).await?;
    let resource = ugoite_iceberg::authorization::ResourceRef {
        kind: parse_resource_kind(&kind)?,
        id: resource_id,
        parent: None,
    };
    Authorizer::new(state.service.operator().clone())
        .set_policy(&space_id, actor, &resource, policy.clone())
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        serde_json::to_value(policy).map_err(|error| auth_error(error.into()))?,
    ))
}

async fn list_spaces(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    let ids = state
        .service
        .list_space_ids()
        .await
        .map_err(ApiError::from_core)?;
    let mut items = Vec::new();
    for id in ids {
        let Ok(principal_id) = principal_for_space(&state, &id, &identity).await else {
            continue;
        };
        if Authorizer::new(state.service.operator().clone())
            .require(&id, principal_id, Action::Read, None)
            .await
            .is_err()
        {
            continue;
        }
        let value = sanitize_space_response(
            state
                .service
                .get_space(&id)
                .await
                .map_err(ApiError::from_core)?,
        );
        items.push(value);
    }
    Ok(Json(Value::Array(items)))
}

#[derive(Deserialize)]
struct SpaceCreate {
    name: String,
}

async fn create_space(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<SpaceCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_recent_passkey(&identity)?;
    validate_id(&payload.name, "space_id")?;
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "node admin role is required to create a Space",
        ));
    }
    let principal_id = Uuid::now_v7();
    let space_uid = state
        .service
        .create_space_for_principal(&payload.name, principal_id, &identity.display_name)
        .await
        .map_err(ApiError::from_core)?;
    state
        .identity
        .bind_local_owner(space_uid, principal_id, identity.account_id)
        .await
        .map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": space_uid,
            "slug": payload.name,
            "space_uid": space_uid,
            "name": payload.name,
            "path": state.workspace(&space_uid.to_string())
        })),
    ))
}

async fn get_space(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let value = sanitize_space_response(
        state
            .service
            .get_space(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    );
    Ok(Json(value))
}

#[derive(Default, Deserialize)]
struct SpaceHealthQuery {
    #[serde(default)]
    checkpoint: Vec<String>,
}

async fn space_health(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<SpaceHealthQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    state
        .service
        .space_health(&space_id, &query.checkpoint)
        .await
        .map(Json)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Space health evidence is unavailable",
            )
        })
}

#[derive(Deserialize)]
struct CheckpointCreate {
    name: String,
}

async fn create_checkpoint(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<CheckpointCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    let value = state
        .service
        .create_named_checkpoint(&space_id, &payload.name)
        .await
        .map_err(ApiError::from_core)?;
    Ok((StatusCode::CREATED, Json(value)))
}

#[derive(Deserialize)]
struct CheckpointDiffQuery {
    from: String,
    to: String,
}

async fn checkpoint_diff(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<CheckpointDiffQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    state
        .service
        .diff_checkpoints_authorized_for_principals(&space_id, &query.from, &query.to, &principals)
        .await
        .map(Json)
        .map_err(ApiError::from_core)
}

async fn patch_space(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    let value = sanitize_space_response(
        state
            .service
            .patch_space(&space_id, &payload)
            .await
            .map_err(ApiError::from_core)?,
    );
    Ok(Json(value))
}

#[derive(Default, Deserialize)]
struct AuditQuery {
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_audit_limit")]
    limit: usize,
    action: Option<String>,
    actor_principal_id: Option<String>,
    outcome: Option<String>,
}

fn default_audit_limit() -> usize {
    100
}

async fn list_audit_events(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    audit::list_audit_events(
        state.service.operator(),
        &space_id,
        AuditListOptions {
            offset: query.offset,
            limit: query.limit,
            action: query.action,
            actor_principal_id: query.actor_principal_id,
            outcome: query.outcome,
        },
    )
    .await
    .map(Json)
    .map_err(ApiError::from_core)
}

async fn get_preferences(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .get_user_preferences(&identity.account_id.to_string())
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn patch_preferences(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .patch_user_preferences(&identity.account_id.to_string(), &payload)
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn list_members(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let members = authorization
        .memberships
        .values()
        .filter_map(|membership| {
            authorization
                .principals
                .get(&membership.principal_id)
                .map(|principal| {
                    json!({
                        "principal": principal,
                        "role": membership.role,
                        "created_at": membership.created_at,
                    })
                })
        })
        .collect();
    Ok(Json(Value::Array(members)))
}

#[derive(Deserialize)]
struct MemberInvite {
    label: String,
    role: String,
}

async fn invite_member(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<MemberInvite>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    require_recent_passkey(&identity)?;
    parse_space_role(&payload.role)?;
    let space_uid = state
        .service
        .space_uid(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let (invitation, token) = state
        .identity
        .issue_invitation(
            identity.account_id,
            &payload.label,
            Some(space_uid),
            Some(payload.role),
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    let (issuer, _) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "invitation_id": invitation.invitation_id,
            "expires_at": invitation.expires_at,
            "invitation_url": format!("{}/spaces/join#token={token}", issuer.trim_end_matches('/')),
        })),
    ))
}

#[derive(Deserialize)]
struct MemberRoleUpdate {
    role: String,
}

async fn update_member_role(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, principal_id)): Path<(String, Uuid)>,
    Json(payload): Json<MemberRoleUpdate>,
) -> ApiResult<Json<Value>> {
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    require_recent_passkey(&identity)?;
    let actor = principal_for_space(&state, &space_id, &identity).await?;
    let role = parse_space_role(&payload.role)?;
    Authorizer::new(state.service.operator().clone())
        .change_role(&space_id, actor, principal_id, role.clone())
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok(Json(json!({"principal_id": principal_id, "role": role})))
}

async fn revoke_member(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, principal_id)): Path<(String, Uuid)>,
) -> ApiResult<Json<Value>> {
    reconcile_recovery_fences(&state, &space_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    require_recent_passkey(&identity)?;
    let actor = principal_for_space(&state, &space_id, &identity).await?;
    Authorizer::new(state.service.operator().clone())
        .revoke_principal(&space_id, actor, principal_id)
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok(Json(
        json!({"principal_id": principal_id, "state": "revoked"}),
    ))
}

#[derive(Deserialize)]
struct SqlSessionCreate {
    sql: String,
    #[serde(default)]
    parameters: serde_json::Map<String, Value>,
    #[serde(default)]
    parameter_types: BTreeMap<String, String>,
}

async fn create_sql_session(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<SqlSessionCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    if payload.sql.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "sql is required",
        ));
    }
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .service
                .create_sql_session_authorized_for_principals_with_parameters(
                    &space_id,
                    &principals,
                    &payload.sql,
                    payload.parameters,
                    payload.parameter_types,
                )
                .await
                .map_err(ApiError::from_core)?,
        ),
    ))
}

async fn get_sql_session(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, session_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    Ok(Json(
        state
            .service
            .get_sql_session_authorized_for_principals(&space_id, &session_id, &principals)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn get_sql_session_count(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, session_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    Ok(Json(json!({
        "count": state
            .service
            .get_sql_session_count_authorized_for_principals(&space_id, &session_id, &principals)
            .await
            .map_err(ApiError::from_core)?,
    })))
}

#[derive(Deserialize)]
struct SqlSessionRowsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

async fn get_sql_session_rows(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, session_id)): Path<(String, String)>,
    Query(query): Query<SqlSessionRowsQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let offset = query.offset.unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(ugoite_iceberg::sql_session::DEFAULT_PAGE_SIZE);
    validate_sql_session_page_request(offset, limit)?;
    Ok(Json(
        state
            .service
            .get_sql_session_rows_authorized_for_principals(
                &space_id,
                &session_id,
                &principals,
                offset,
                limit,
            )
            .await
            .map_err(ApiError::from_core)?,
    ))
}

fn validate_sql_session_page_request(offset: usize, limit: usize) -> ApiResult<()> {
    let page_end = offset.checked_add(limit);
    if limit == 0
        || limit > ugoite_iceberg::sql_session::MAX_PAGE_SIZE
        || page_end.is_none_or(|end| end > ugoite_iceberg::sql_session::MAX_PAGE_SIZE)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "SQL session page range exceeds the configured maximum",
        ));
    }
    Ok(())
}

async fn test_connection(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    validate_id(&space_id, "space_id")?;
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    let config_value = payload
        .get("storage_config")
        .cloned()
        .unwrap_or_else(|| payload.clone());
    let config: space::StorageConnectionTestConfig =
        serde_json::from_value(config_value).map_err(|_| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "storage_config.uri is required",
            )
        })?;
    Ok(Json(
        state
            .service
            .test_storage_connection(&config)
            .await
            .map_err(storage_connection_error)?,
    ))
}

fn storage_connection_error(error: anyhow::Error) -> ApiError {
    if error.downcast_ref::<AppError>().is_some() {
        return ApiError::from_core(error);
    }
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
}

fn sanitize_space_response(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        for key in ["hmac_key", "hmac_key_id", "last_rotation"] {
            object.remove(key);
        }
        if let Some(storage_config) = object.get_mut("storage_config") {
            redact_sensitive_storage_config(storage_config);
        }
        if let Some(settings) = object.get_mut("settings").and_then(Value::as_object_mut) {
            for key in MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS {
                settings.remove(*key);
            }
        }
    }
    value
}

fn redact_sensitive_storage_config(value: &mut Value) {
    const REDACTED_KEYS: &[&str] = &[
        "access_key",
        "client_secret",
        "credential",
        "credentials",
        "password",
        "secret",
        "secret_access_key",
        "secret_key",
        "session_token",
        "token",
    ];

    match value {
        Value::Object(object) => {
            for key in REDACTED_KEYS {
                object.remove(*key);
            }
            for nested in object.values_mut() {
                redact_sensitive_storage_config(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_storage_config(item);
            }
        }
        _ => {}
    }
}

#[derive(Deserialize)]
struct EntryCreate {
    id: Option<String>,
    #[serde(alias = "content")]
    markdown: String,
}

async fn create_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<EntryCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let entry_id = payload.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_id(&entry_id, "entry_id")?;
    let created = state
        .service
        .create_entry_authorized_for_principals(
            &space_id,
            &entry_id,
            &payload.markdown,
            &principal_id.to_string(),
            &principals,
        )
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": entry_id, "revision_id": created["revision_id"]})),
    ))
}

async fn list_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<EntryListQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    Ok(Json(Value::Array(
        state
            .service
            .list_entries_authorized_for_principals(
                &space_id,
                &principals,
                query
                    .limit
                    .unwrap_or(100)
                    .min(ugoite_iceberg::MAX_NORMAL_READ_ROWS),
                query.offset.unwrap_or(0),
            )
            .await
            .map_err(ApiError::from_core)?,
    )))
}

#[derive(Deserialize)]
struct EntryListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Deserialize)]
struct EntryOptionsQuery {
    form: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

async fn entry_options(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<EntryOptionsQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let options = state
        .service
        .list_entry_options_authorized_for_principals(
            &space_id,
            &principals,
            query.form.as_deref(),
            query.q.as_deref(),
            query.limit.unwrap_or(8).min(20),
        )
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        serde_json::to_value(options).map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn get_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Query(query): Query<EntryReadQuery>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    validate_id(&entry_id, "entry_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let mut value = if let Some(checkpoint) = query.checkpoint.as_deref() {
        state
            .service
            .entry_at_checkpoint_authorized_for_principals(
                &space_id,
                &entry_id,
                checkpoint,
                &principals,
            )
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .get_entry_authorized_for_principals(&space_id, &entry_id, &principals)
            .await
            .map_err(ApiError::from_core)?
    };
    if let Some(content) = value.get("content").cloned() {
        value["markdown"] = content;
    }
    Ok(Json(value))
}

#[derive(Default, Deserialize)]
struct EntryReadQuery {
    checkpoint: Option<String>,
}

#[derive(Deserialize)]
struct EntryUpdate {
    markdown: String,
    parent_revision_id: Option<String>,
}

async fn update_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<EntryUpdate>,
) -> ApiResult<Json<Value>> {
    let principal_id = require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Update,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    validate_id(&entry_id, "entry_id")?;
    let value = state
        .service
        .update_entry_authorized_for_principals(
            &space_id,
            &entry_id,
            &payload.markdown,
            payload.parent_revision_id.as_deref(),
            &principal_id.to_string(),
            &principals,
        )
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        json!({"id": entry_id, "revision_id": value["revision_id"]}),
    ))
}

async fn delete_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Query(query): Query<EntryDeleteQuery>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Delete,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    validate_id(&entry_id, "entry_id")?;
    state
        .service
        .delete_entry(&space_id, &entry_id, query.hard_delete.unwrap_or(false))
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(json!({"id": entry_id, "status": "deleted"})))
}

#[derive(Deserialize)]
struct EntryDeleteQuery {
    hard_delete: Option<bool>,
}

async fn entry_history(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Query(query): Query<EntryReadQuery>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    validate_id(&entry_id, "entry_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let mut history = if let Some(checkpoint) = query.checkpoint.as_deref() {
        state
            .service
            .entry_history_at_checkpoint(&space_id, &entry_id, checkpoint, &principals)
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .entry_history(&space_id, &entry_id)
            .await
            .map_err(ApiError::from_core)?
    };
    history["access_policy_history"] = serde_json::to_value(
        Authorizer::new(state.service.operator().clone())
            .resource_policy_history(
                &space_id,
                &ugoite_iceberg::authorization::ResourceRef {
                    kind: ResourceKind::Entry,
                    id: entry_id,
                    parent: None,
                },
            )
            .await
            .map_err(ApiError::from_core)?,
    )
    .map_err(|error| ApiError::from_core(error.into()))?;
    Ok(Json(history))
}

async fn entry_revision(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id, revision_id)): Path<(String, String, String)>,
    Query(query): Query<EntryReadQuery>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    validate_id(&entry_id, "entry_id")?;
    validate_id(&revision_id, "revision_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let mut revision = if let Some(checkpoint) = query.checkpoint.as_deref() {
        state
            .service
            .entry_revision_at_checkpoint(
                &space_id,
                &entry_id,
                &revision_id,
                checkpoint,
                &principals,
            )
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .entry_revision(&space_id, &entry_id, &revision_id)
            .await
            .map_err(ApiError::from_core)?
    };
    let policy_history = Authorizer::new(state.service.operator().clone())
        .resource_policy_history(
            &space_id,
            &ugoite_iceberg::authorization::ResourceRef {
                kind: ResourceKind::Entry,
                id: entry_id,
                parent: None,
            },
        )
        .await
        .map_err(ApiError::from_core)?;
    let revision_time = revision
        .get("timestamp")
        .and_then(Value::as_f64)
        .and_then(|timestamp| chrono::DateTime::from_timestamp_millis((timestamp * 1000.0) as i64));
    revision["access_policy"] = serde_json::to_value(
        policy_history
            .iter()
            .rev()
            .find(|policy| {
                revision_time.is_none_or(|revision_time| {
                    chrono::DateTime::parse_from_rfc3339(&policy.changed_at)
                        .map(|changed| changed.with_timezone(&chrono::Utc) <= revision_time)
                        .unwrap_or(false)
                })
            })
            .map(|revision| &revision.policy),
    )
    .map_err(|error| ApiError::from_core(error.into()))?;
    Ok(Json(revision))
}

#[derive(Deserialize)]
struct RestoreEntry {
    revision_id: String,
    #[serde(default)]
    checkpoint: Option<String>,
}

async fn restore_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<RestoreEntry>,
) -> ApiResult<Json<Value>> {
    let principal_id = require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Update,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    validate_id(&entry_id, "entry_id")?;
    validate_id(&payload.revision_id, "revision_id")?;
    let value = if let Some(checkpoint) = payload.checkpoint.as_deref() {
        state
            .service
            .restore_entry_from_checkpoint_authorized_for_principals(
                &space_id,
                &entry_id,
                &payload.revision_id,
                checkpoint,
                &principal_id.to_string(),
                &principals,
            )
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .restore_entry_authorized_for_principals(
                &space_id,
                &entry_id,
                &payload.revision_id,
                &principal_id.to_string(),
                &principals,
            )
            .await
            .map_err(ApiError::from_core)?
    };
    Ok(Json(value))
}

async fn list_forms(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let forms = state
        .service
        .list_forms(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(Value::Array(
        state
            .service
            .filter_json_resources_authorized_for_principals(
                &space_id,
                &principals,
                ResourceKind::Form,
                "name",
                forms,
            )
            .await
            .map_err(ApiError::from_core)?,
    )))
}

async fn form_types(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        serde_json::to_value(
            form::list_column_types()
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn get_form(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, form_name)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::Form,
        &form_name,
    )
    .await?;
    validate_id(&form_name, "form_name")?;
    Ok(Json(
        state
            .service
            .get_form(&space_id, &form_name)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn upsert_form(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let form_name = payload.get("name").and_then(Value::as_str).ok_or_else(|| {
        ApiError::from_core(
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Form definition missing 'name' field",
            )
            .into(),
        )
    })?;
    let existing_form = state
        .service
        .list_forms(&space_id)
        .await
        .map_err(ApiError::from_core)?
        .into_iter()
        .any(|form| form.get("name").and_then(Value::as_str) == Some(form_name));
    if existing_form {
        require_resource_action(
            &state,
            &space_id,
            &identity,
            Action::Update,
            ResourceKind::Form,
            form_name,
        )
        .await?;
    } else {
        require_space_action(&state, &space_id, &identity, Action::Create).await?;
    }
    state
        .service
        .upsert_form(&space_id, &payload)
        .await
        .map_err(ApiError::from_core)?;
    Ok((StatusCode::CREATED, Json(payload)))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

async fn search_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .search_entries_authorized_for_principals(
                    &space_id,
                    &principals,
                    &query.q,
                    query
                        .limit
                        .unwrap_or(100)
                        .min(ugoite_iceberg::MAX_NORMAL_READ_ROWS),
                )
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn query_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let filter = payload.get("filter").cloned().unwrap_or(payload);
    Ok(Json(Value::Array(
        state
            .service
            .query_entries_authorized_for_principals(&space_id, &principals, &filter)
            .await
            .map_err(ApiError::from_core)?,
    )))
}

async fn list_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let statements = state
        .service
        .list_saved_sql_authorized_for_principals(&space_id, &principals)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(Value::Array(statements)))
}

async fn create_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<saved_sql::SqlPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_action(&state, &space_id, &identity, Action::Create).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let id = Uuid::new_v4().to_string();
    let value = state
        .service
        .create_saved_sql(&space_id, &id, &payload, &principal_id.to_string())
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "revision_id": value["revision_id"]})),
    ))
}

async fn get_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::SavedSql,
        &sql_id,
    )
    .await?;
    Ok(Json(
        state
            .service
            .get_saved_sql(&space_id, &sql_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn update_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, sql_id)): Path<(String, String)>,
    Json(payload): Json<saved_sql::SqlUpdatePayload>,
) -> ApiResult<Json<Value>> {
    let principal_id = require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Update,
        ResourceKind::SavedSql,
        &sql_id,
    )
    .await?;
    validate_id(&sql_id, "sql_id")?;
    let parent_revision_id = payload.parent_revision_id.clone();
    let payload = payload.into_sql_payload();
    Ok(Json(
        state
            .service
            .update_saved_sql(
                &space_id,
                &sql_id,
                &payload,
                &parent_revision_id,
                &principal_id.to_string(),
            )
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn delete_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Delete,
        ResourceKind::SavedSql,
        &sql_id,
    )
    .await?;
    validate_id(&sql_id, "sql_id")?;
    state
        .service
        .delete_saved_sql(&space_id, &sql_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let field = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "file is required"))?;
    let name = field.file_name().unwrap_or("asset").to_string();
    let media_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = field
        .bytes()
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let value = state
        .service
        .save_asset_with_media_type(&space_id, &name, &bytes, &media_type)
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(value).map_err(|error| ApiError::from_core(error.into()))?),
    ))
}

#[derive(Deserialize)]
struct AssetReadQuery {
    form: Option<String>,
    entry_id: Option<String>,
}

async fn get_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, asset_id)): Path<(String, String)>,
    Query(query): Query<AssetReadQuery>,
) -> ApiResult<Response> {
    let form_name = query.form.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "asset reads require a containing Form and Entry context",
        )
    })?;
    let entry_id = query.entry_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "asset reads require a containing Form and Entry context",
        )
    })?;
    validate_id(&asset_id, "asset_id")?;
    validate_id(&entry_id, "entry_id")?;
    let principal_id = require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Read,
        ResourceKind::Entry,
        &entry_id,
    )
    .await?;
    let entry_parent = ugoite_iceberg::authorization::ResourceRef {
        kind: ResourceKind::Entry,
        id: entry_id.clone(),
        parent: None,
    };
    state
        .service
        .require_resource_action(
            &space_id,
            principal_id,
            Action::Read,
            ResourceKind::Asset,
            &asset_id,
            Some(entry_parent.clone()),
        )
        .await
        .map_err(ApiError::from_core)?;
    if let Some(actor_principal_id) = identity.token_actor_principal_id {
        if actor_principal_id != principal_id {
            state
                .service
                .require_resource_action(
                    &space_id,
                    actor_principal_id,
                    Action::Read,
                    ResourceKind::Asset,
                    &asset_id,
                    Some(entry_parent),
                )
                .await
                .map_err(ApiError::from_core)?;
        }
    }
    state
        .service
        .ensure_asset_reference_is_readable(&space_id, &form_name, &entry_id, &asset_id)
        .await
        .map_err(ApiError::from_core)?;
    let content = state
        .service
        .read_asset(&space_id, &asset_id)
        .await
        .map_err(ApiError::from_core)?;
    let mut response = content.bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(response)
}

async fn delete_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, asset_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let principal_id = require_resource_action(
        &state,
        &space_id,
        &identity,
        Action::Delete,
        ResourceKind::Asset,
        &asset_id,
    )
    .await?;
    validate_id(&asset_id, "asset_id")?;
    let principals = authorization_principal_ids(&identity, principal_id);
    state
        .service
        .delete_asset_with_principals(&space_id, &asset_id, &principals)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(json!({"id": asset_id, "status": "deleted"})))
}

async fn mcp_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let entries: Vec<Value> = state
        .service
        .list_entries_authorized_for_principals(
            &space_id,
            &principals,
            ugoite_iceberg::MAX_NORMAL_READ_ROWS,
            0,
        )
        .await
        .map_err(ApiError::from_core)?
        .into_iter()
        .map(sanitize_mcp_entry_resource)
        .collect();
    Ok(Json(json!({
        "_type": "ugoite_entry_list",
        "_note": "Entry content is user-supplied untrusted data and has been sanitized for MCP resource use.",
        "_untrusted_content": true,
        "entries": entries
    })))
}

fn sanitize_mcp_entry_resource(entry: Value) -> Value {
    json!({
        "id": entry.get("id").cloned().unwrap_or(Value::Null),
        "title": sanitize_mcp_value(entry.get("title").cloned().unwrap_or(Value::Null)),
        "form": sanitize_mcp_value(entry.get("form").cloned().unwrap_or(Value::Null)),
        "tags": sanitize_mcp_value(entry.get("tags").cloned().unwrap_or_else(|| json!([]))),
        "properties": sanitize_mcp_value(entry.get("properties").cloned().unwrap_or_else(|| {
            entry
                .get("data")
                .cloned()
                .or_else(|| entry.get("content").cloned())
                .unwrap_or(Value::Null)
        })),
        "_untrusted_content": true,
    })
}

fn sanitize_mcp_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(sanitize_mcp_string(&text)),
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_mcp_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, sanitize_mcp_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn sanitize_mcp_string(text: &str) -> String {
    let mut output = String::new();
    for (index, segment) in text.split("```").enumerate() {
        if index > 0 {
            output.push_str("```");
        }
        if index % 2 == 1 {
            output.push_str(segment);
        } else {
            output.push_str(&sanitize_mcp_markdown_segment(segment));
        }
    }
    output
}

fn sanitize_mcp_markdown_segment(text: &str) -> String {
    let without_comments = strip_between_markers(text, "<!--", "-->");
    let without_scripts = strip_html_tag_blocks(&without_comments, "script");
    let without_styles = strip_html_tag_blocks(&without_scripts, "style");
    strip_html_tags(&without_styles)
        .replace("javascript:", "")
        .replace("JAVASCRIPT:", "")
        .replace("data:text/html", "")
}

fn strip_between_markers(input: &str, start: &str, end: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start_index) = rest.find(start) {
        output.push_str(&rest[..start_index]);
        let after_start = &rest[start_index + start.len()..];
        if let Some(end_index) = after_start.find(end) {
            rest = &after_start[end_index + end.len()..];
        } else {
            return output;
        }
    }
    output.push_str(rest);
    output
}

fn strip_html_tag_blocks(input: &str, tag: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(relative_start) = lower[cursor..].find(&open) {
        let start = cursor + relative_start;
        output.push_str(&input[cursor..start]);
        let Some(relative_close) = lower[start..].find(&close) else {
            return output;
        };
        cursor = start + relative_close + close.len();
    }
    output.push_str(&input[cursor..]);
    output
}

fn strip_html_tags(input: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

pub fn openapi_snapshot() -> Value {
    serde_json::from_str(OPENAPI_JSON).expect("embedded OpenAPI snapshot must be valid JSON")
}

#[cfg(test)]
mod authentication_regression_tests {
    use super::*;
    use axum::{
        body::Body,
        http::Request,
        routing::{get, post, put},
    };
    use tower::ServiceExt;

    fn token_claims(sub: Uuid, actor_principal_id: Option<Uuid>) -> AccessTokenClaims {
        AccessTokenClaims {
            iss: "https://ugoite.example".to_string(),
            node_id: Uuid::now_v7(),
            sub,
            principal_type: if actor_principal_id.is_some() {
                "human".to_string()
            } else {
                "agent".to_string()
            },
            actor_principal_id,
            aud: "https://ugoite.example".to_string(),
            space_uid: Uuid::now_v7(),
            granted_actions: ["read".to_string()].into_iter().collect(),
            actor_chain: Vec::new(),
            exp: chrono::Utc::now().timestamp() + 300,
            iat: chrono::Utc::now().timestamp(),
            jti: Uuid::now_v7(),
            credential_id: Uuid::now_v7(),
            credential_generation: None,
            cnf: Confirmation {
                jkt: "thumbprint".to_string(),
            },
        }
    }

    fn content_identity(principal_id: Uuid, space_uid: Uuid) -> RequestIdentityContext {
        RequestIdentityContext {
            request_identity: RequestIdentity {
                subject: AuthenticatedSubject::HumanAccount {
                    account_id: principal_id,
                },
                actor: Actor::Human {
                    account_id: principal_id,
                },
                credential_id: Uuid::now_v7(),
                authentication_method: RequestAuthenticationMethod::Passkey,
                assurance: AssuranceLevel::PhishingResistant,
                constraints: CredentialConstraints::default(),
                session_id: None,
            },
            account_id: principal_id,
            display_name: "Route test".into(),
            node_admin: false,
            token_principal_id: Some(principal_id),
            token_actor_principal_id: None,
            token_space_uid: Some(space_uid),
            token_actions: Some(
                ["read", "create", "update"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            ),
            recent_passkey: true,
            credential_generation: 0,
        }
    }

    #[test]
    fn autonomous_and_delegated_tokens_authenticate_the_agent_credential_subject() {
        let agent = Uuid::now_v7();
        let human = Uuid::now_v7();
        assert_eq!(access_token_agent_id(&token_claims(agent, None)), &agent);
        assert_eq!(
            access_token_agent_id(&token_claims(human, Some(agent))),
            &agent
        );
    }

    #[test]
    fn delegated_permissions_are_an_intersection_and_cli_defaults_are_accepted() {
        let agent = [Action::Read, Action::Update].into_iter().collect();
        let human = [Action::Read, Action::Create].into_iter().collect();
        assert_eq!(
            delegated_agent_actions(&agent, &human),
            [Action::Read].into_iter().collect()
        );
        let defaults = ["read", "create", "update"]
            .into_iter()
            .map(str::to_string)
            .collect();
        validate_action_names(&defaults).expect("CLI default actions are known");
        validate_access_credential_actions(&defaults)
            .expect("CLI default actions require no unavailable approval flow");
    }

    #[test]
    fn sql_session_page_request_rejects_zero_overflow_and_large_windows() {
        assert!(validate_sql_session_page_request(0, 1).is_ok());
        assert!(validate_sql_session_page_request(0, 0).is_err());
        assert!(validate_sql_session_page_request(999, 2).is_err());
        assert!(validate_sql_session_page_request(usize::MAX, 1).is_err());
        assert!(validate_sql_session_page_request(
            0,
            ugoite_iceberg::sql_session::MAX_PAGE_SIZE + 1
        )
        .is_err());
    }

    #[test]
    fn unsupported_form_field_type_changes_are_client_errors_with_actionable_details() {
        let error = ApiError::from_core(
            AppError::form_field_type_change_not_supported("time", "timestamp", "date").into(),
        );

        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            error.detail,
            json!({
                "code": "FORM_FIELD_TYPE_CHANGE_NOT_SUPPORTED",
                "message": "Changing the type of existing Form field 'time' from 'timestamp' to 'date' is not supported; create a new field instead"
            })
        );
    }

    #[test]
    fn asset_delete_conflicts_are_stable_non_internal_http_errors() {
        let visible = ApiError::from_core(
            AppError::conflict(
                ugoite_core::error::ErrorCode::AssetReferenced,
                "Asset is referenced by an authorized entry",
            )
            .into(),
        );
        assert_eq!(visible.status, StatusCode::CONFLICT);
        assert_eq!(visible.detail["code"], "ASSET_REFERENCED");

        let hidden =
            ApiError::from_core(AppError::forbidden("Asset deletion is not permitted").into());
        assert_eq!(hidden.status, StatusCode::FORBIDDEN);
        assert_eq!(hidden.detail["code"], "FORBIDDEN");
        assert_eq!(hidden.detail["message"], "Asset deletion is not permitted");
    }

    #[tokio::test]
    async fn form_type_change_route_returns_422_with_the_workspace_error() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-form-type-change-route")?;
        let principal_id = Uuid::from_u128(1859);
        let space_id = state
            .service
            .create_space_for_principal("form-type-change", principal_id, "Route test")
            .await?
            .to_string();
        let form_id = Uuid::from_u128(1862);
        let original = json!({
            "id": form_id,
            "name": "Meeting",
            "version": 1,
            "fields": {
                "time": {"id": 100, "type": "timestamp", "required": false}
            },
            "allow_extra_attributes": "deny"
        });
        state.service.upsert_form(&space_id, &original).await?;

        let desired = json!({
            "id": form_id,
            "name": "Meeting",
            "version": 1,
            "fields": {
                "time": {"id": 100, "type": "date", "required": false}
            },
            "allow_extra_attributes": "deny"
        });
        let space_uid = state.service.space_uid(&space_id).await?;
        let identity = content_identity(principal_id, space_uid);
        let route = Router::new()
            .route("/spaces/{space_id}/forms", post(upsert_form))
            .layer(Extension(identity))
            .with_state(state.clone());
        let list_form_id = Uuid::from_u128(1863);
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "id": list_form_id,
                    "name": "ListMeeting",
                    "version": 1,
                    "fields": {
                        "labels": {
                            "id": 100,
                            "type": "list",
                            "items": {"type": "string"}
                        }
                    },
                    "allow_extra_attributes": "deny"
                }),
            )
            .await?;
        let list_type_change_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/forms"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "id": list_form_id,
                            "name": "ListMeeting",
                            "version": 1,
                            "fields": {
                                "labels": {
                                    "id": 100,
                                    "type": "list",
                                    "items": {"type": "integer"}
                                }
                            },
                            "allow_extra_attributes": "deny"
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("list item type change response");
        assert_eq!(
            list_type_change_response.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let list_type_change_body =
            axum::body::to_bytes(list_type_change_response.into_body(), usize::MAX).await?;
        let list_type_change_body: Value = serde_json::from_slice(&list_type_change_body)?;
        assert_eq!(
            list_type_change_body["code"],
            "FORM_FIELD_TYPE_CHANGE_NOT_SUPPORTED"
        );
        assert!(list_type_change_body["message"]
            .as_str()
            .is_some_and(|message| message.contains("labels")));
        let response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/forms"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(desired.to_string()))?,
            )
            .await
            .expect("Form route response");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body: Value = serde_json::from_slice(&body)?;
        assert_eq!(body["code"], "FORM_FIELD_TYPE_CHANGE_NOT_SUPPORTED");
        assert_eq!(
            body["message"],
            "Changing the type of existing Form field 'time' from 'timestamp' to 'date' is not supported; create a new field instead"
        );
        let stored = state.service.get_form(&space_id, "Meeting").await?;
        assert_eq!(stored["fields"]["time"]["type"], "timestamp");

        let removal = json!({
            "id": form_id,
            "name": "Meeting",
            "version": 1,
            "fields": {},
            "allow_extra_attributes": "deny"
        });
        let removal_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/forms"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(removal.to_string()))?,
            )
            .await
            .expect("Form removal response");
        assert_eq!(removal_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let removal_body = axum::body::to_bytes(removal_response.into_body(), usize::MAX).await?;
        let removal_body: Value = serde_json::from_slice(&removal_body)?;
        assert_eq!(removal_body["code"], "FORM_FIELD_REMOVAL_NOT_SUPPORTED");
        assert!(removal_body["message"].as_str().is_some_and(|message| {
            message.contains("time") && message.contains("add a new field")
        }));
        let stored_after_removal = state.service.get_form(&space_id, "Meeting").await?;
        assert_eq!(stored_after_removal["fields"]["time"]["type"], "timestamp");
        Ok(())
    }

    #[tokio::test]
    async fn entry_routes_keep_input_errors_typed_and_create_atomic() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-entry-create-contract")?;
        let principal_id = Uuid::from_u128(1872);
        let space_id = state
            .service
            .create_space_for_principal("entry-create-contract", principal_id, "Route test")
            .await?
            .to_string();
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "name": "Entry",
                    "fields": {
                        "Body": {"type": "markdown"},
                        "test number": {"type": "double"},
                        "ts": {"type": "timestamp"}
                    },
                    "allow_extra_attributes": "allow_columns"
                }),
            )
            .await?;
        let space_uid = state.service.space_uid(&space_id).await?;
        let route = Router::new()
            .route("/spaces/{space_id}/entries", post(create_entry))
            .route("/spaces/{space_id}/entries/{entry_id}", put(update_entry))
            .route(
                "/spaces/{space_id}/entries/{entry_id}/history",
                get(entry_history),
            )
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state.clone());

        let invalid_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/entries"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "id": "invalid-entry",
                            "markdown": "---\nform: Entry\n---\n# Invalid\n\n## Body\nBody\n\n## test number\n0\n\n## ts\nnot-a-timestamp"
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("invalid entry response");
        assert_eq!(invalid_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let invalid_body = axum::body::to_bytes(invalid_response.into_body(), usize::MAX).await?;
        let invalid_body: Value = serde_json::from_slice(&invalid_body)?;
        assert_eq!(invalid_body["code"], "FORM_VALIDATION_FAILED");
        assert!(state.service.list_entries(&space_id).await?.is_empty());

        let missing_form_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/entries"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "id": "missing-form-entry",
                            "markdown": "---\nform: Missing\n---\n# Missing form"
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("missing form response");
        assert_eq!(missing_form_response.status(), StatusCode::NOT_FOUND);
        let missing_form_body =
            axum::body::to_bytes(missing_form_response.into_body(), usize::MAX).await?;
        let missing_form_body: Value = serde_json::from_slice(&missing_form_body)?;
        assert_eq!(missing_form_body["code"], "FORM_NOT_FOUND");
        assert!(state.service.list_entries(&space_id).await?.is_empty());

        let success_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/entries"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "id": "created-entry",
                            "markdown": "---\nform: Entry\n---\n# Created\n\n## Body\nBody\n\n## test number\n0\n\n## ts\n2026-08-21T10:48"
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("successful entry response");
        assert_eq!(success_response.status(), StatusCode::CREATED);
        let success_body = axum::body::to_bytes(success_response.into_body(), usize::MAX).await?;
        let success_body: Value = serde_json::from_slice(&success_body)?;
        assert_eq!(success_body["id"], "created-entry");
        let created_revision_id = success_body["revision_id"]
            .as_str()
            .expect("create response revision id");

        let invalid_update_response = route
            .clone()
            .oneshot(
                Request::put(format!(
                    "/spaces/{space_id}/entries/created-entry"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "markdown": "---\nform: Entry\n---\n# Created\n\n## Body\nBody\n\n## test number\n0\n\n## ts\nnot-a-timestamp",
                        "parent_revision_id": created_revision_id
                    })
                    .to_string(),
                ))?,
            )
            .await
            .expect("invalid entry update response");
        assert_eq!(
            invalid_update_response.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let invalid_update_body =
            axum::body::to_bytes(invalid_update_response.into_body(), usize::MAX).await?;
        let invalid_update_body: Value = serde_json::from_slice(&invalid_update_body)?;
        assert_eq!(invalid_update_body["code"], "FORM_VALIDATION_FAILED");

        let history_response = route
            .oneshot(
                Request::get(format!("/spaces/{space_id}/entries/created-entry/history"))
                    .body(Body::empty())?,
            )
            .await
            .expect("entry history response");
        assert_eq!(history_response.status(), StatusCode::OK);
        let history_body = axum::body::to_bytes(history_response.into_body(), usize::MAX).await?;
        let history_body: Value = serde_json::from_slice(&history_body)?;
        let revisions = history_body["revisions"]
            .as_array()
            .expect("revision array");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0]["revision_id"], created_revision_id);
        Ok(())
    }

    #[tokio::test]
    async fn asset_reference_input_errors_are_422_and_do_not_publish_revisions(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-asset-reference-validation")?;
        let principal_id = Uuid::from_u128(1877);
        let space_id = state
            .service
            .create_space_for_principal("asset-reference-validation", principal_id, "Route test")
            .await?
            .to_string();
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "name": "AssetReview",
                    "fields": {
                        "thumbnail": {"type": "asset_reference", "required": false},
                        "documents": {
                            "type": "list",
                            "required": false,
                            "items": {"type": "asset_reference"}
                        }
                    },
                    "allow_extra_attributes": "deny"
                }),
            )
            .await?;
        let space_uid = state.service.space_uid(&space_id).await?;
        let route = Router::new()
            .route("/spaces/{space_id}/entries", post(create_entry))
            .route("/spaces/{space_id}/entries/{entry_id}", put(update_entry))
            .route(
                "/spaces/{space_id}/entries/{entry_id}/history",
                get(entry_history),
            )
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state.clone());

        let reference = serde_json::to_value(
            state
                .service
                .save_asset_with_media_type(&space_id, "report.pdf", b"report", "application/pdf")
                .await?,
        )?;
        let markdown = |entry_id: &str, thumbnail: Value, documents: Value| {
            format!(
                "---\nform: AssetReview\n---\n# {entry_id}\n\n## thumbnail\n{}\n\n## documents\n{}\n",
                serde_json::to_string(&thumbnail).expect("thumbnail JSON"),
                serde_json::to_string(&documents).expect("documents JSON"),
            )
        };

        let invalid_requests = [
            (
                "invalid-asset-scalar",
                json!({
                    "asset_id": "01900000-0000-7000-8000-000000000187",
                    "name": "report.pdf",
                    "media_type": "application/pdf",
                    "size_bytes": 10,
                    "sha256": "a".repeat(64),
                    "object_key": "forbidden"
                }),
                json!([]),
            ),
            ("invalid-asset-null-item", Value::Null, json!([Value::Null])),
            (
                "invalid-asset-duplicate",
                Value::Null,
                json!([reference.clone(), reference.clone()]),
            ),
        ];
        for (entry_id, thumbnail, documents) in invalid_requests {
            let response = route
                .clone()
                .oneshot(
                    Request::post(format!("/spaces/{space_id}/entries"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({"id": entry_id, "markdown": markdown(entry_id, thumbnail, documents)})
                                .to_string(),
                        ))?,
                )
                .await
                .expect("invalid AssetReference response");
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
            let body: Value = serde_json::from_slice(&body)?;
            assert!(matches!(
                body["code"].as_str(),
                Some("INVALID_INPUT" | "FORM_VALIDATION_FAILED")
            ));
            let diagnostic = format!("{} {}", body["message"], body["detail"]);
            assert!(diagnostic.contains("thumbnail") || diagnostic.contains("documents"));
        }
        assert!(state.service.list_entries(&space_id).await?.is_empty());

        let valid_id = "valid-asset-entry";
        let created = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/entries"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "id": valid_id,
                            "markdown": markdown(valid_id, reference.clone(), json!([reference.clone()]))
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("valid AssetReference response");
        assert_eq!(created.status(), StatusCode::CREATED);
        let created_body = axum::body::to_bytes(created.into_body(), usize::MAX).await?;
        let created_body: Value = serde_json::from_slice(&created_body)?;
        let revision_id = created_body["revision_id"].as_str().expect("revision id");

        let invalid_update = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/entries/{valid_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "markdown": markdown(valid_id, reference.clone(), json!([Value::Null])),
                            "parent_revision_id": revision_id
                        })
                        .to_string(),
                    ))?,
            )
            .await
            .expect("invalid AssetReference update response");
        assert_eq!(invalid_update.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let history = route
            .oneshot(
                Request::get(format!("/spaces/{space_id}/entries/{valid_id}/history"))
                    .body(Body::empty())?,
            )
            .await
            .expect("AssetReference history response");
        assert_eq!(history.status(), StatusCode::OK);
        let history_body = axum::body::to_bytes(history.into_body(), usize::MAX).await?;
        let history_body: Value = serde_json::from_slice(&history_body)?;
        assert_eq!(history_body["revisions"].as_array().map(Vec::len), Some(1));
        assert_eq!(history_body["revisions"][0]["revision_id"], revision_id);
        Ok(())
    }

    #[tokio::test]
    async fn saved_sql_update_contract_preserves_revision_checks() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-saved-sql-update-contract")?;
        let principal_id = Uuid::from_u128(1873);
        let space_id = state
            .service
            .create_space_for_principal("saved-sql-update-contract", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let route = Router::new()
            .route("/spaces/{space_id}/sql", post(create_sql))
            .route("/spaces/{space_id}/sql/{sql_id}", put(update_sql))
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state.clone());

        let create_response = route
            .clone()
            .oneshot(
                Request::post(format!("/spaces/{space_id}/sql"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Saved query",
                            "kind": "user-query",
                            "sql": "SELECT 1",
                            "variables": []
                        })
                        .to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = axum::body::to_bytes(create_response.into_body(), usize::MAX).await?;
        let create_body: Value = serde_json::from_slice(&create_body)?;
        let sql_id = create_body["id"].as_str().expect("SQL id").to_string();
        let first_revision = create_body["revision_id"]
            .as_str()
            .expect("initial SQL revision")
            .to_string();

        let update_body = |parent_revision_id: &str| {
            json!({
                "name": "Saved query updated",
                "kind": "user-query",
                "sql": "SELECT 2",
                "variables": [],
                "parent_revision_id": parent_revision_id,
            })
        };

        let update_without_revision = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Saved query updated",
                            "kind": "user-query",
                            "sql": "SELECT 2",
                            "variables": []
                        })
                        .to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(
            update_without_revision.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );

        let blank_revision = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update_body("   ").to_string()))?,
            )
            .await?;
        assert_eq!(blank_revision.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let first_update = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update_body(&first_revision).to_string()))?,
            )
            .await?;
        assert_eq!(first_update.status(), StatusCode::OK);
        let body = axum::body::to_bytes(first_update.into_body(), usize::MAX).await?;
        let second_revision = serde_json::from_slice::<Value>(&body)?["revision_id"]
            .as_str()
            .expect("second SQL revision")
            .to_string();

        let stale_update = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update_body(&first_revision).to_string()))?,
            )
            .await?;
        assert_eq!(stale_update.status(), StatusCode::CONFLICT);
        let stale_body = axum::body::to_bytes(stale_update.into_body(), usize::MAX).await?;
        assert_eq!(
            serde_json::from_slice::<Value>(&stale_body)?["code"],
            "REVISION_CONFLICT"
        );

        let valid_update = route
            .clone()
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(update_body(&second_revision).to_string()))?,
            )
            .await?;
        assert_eq!(valid_update.status(), StatusCode::OK);

        let unknown_field = route
            .oneshot(
                Request::put(format!("/spaces/{space_id}/sql/{sql_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "name": "Saved query",
                            "kind": "user-query",
                            "sql": "SELECT 3",
                            "variables": [],
                            "parent_revision_id": second_revision,
                            "author": "unexpected"
                        })
                        .to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(unknown_field.status(), StatusCode::UNPROCESSABLE_ENTITY);
        Ok(())
    }

    #[tokio::test]
    async fn invitation_finalization_converges_after_space_membership_commit() -> anyhow::Result<()>
    {
        let state = AppState::new_for_tests("memory://server-invitation-saga")?;
        state.initialize_node().await?;
        let owner_account_id = Uuid::now_v7();
        let owner_principal_id = Uuid::now_v7();
        let invited_account_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("invitation-saga", owner_principal_id, "Owner")
            .await?;
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid,
                principal_id: owner_principal_id,
                node_account_id: owner_account_id,
                binding_method: BindingMethod::Setup,
            })
            .await?;
        let principal_id = Uuid::now_v7();
        let backup_owner_principal_id = Uuid::now_v7();
        let account = HumanAccount {
            account_id: invited_account_id,
            display_name: "Invited viewer".to_string(),
            status: AccountStatus::Active,
            created_at: chrono::Utc::now().to_rfc3339(),
            node_roles: BTreeSet::new(),
            credential_generation: 0,
        };
        let invitation = AccountInvitation {
            invitation_id: Uuid::now_v7(),
            token_hash: "test".to_string(),
            display_name: account.display_name.clone(),
            space_uid: Some(space_uid),
            role: Some("viewer".to_string()),
            expires_at: chrono::Utc::now().to_rfc3339(),
            acceptance: Some(
                ugoite_identity::node_identity::InvitationAcceptance::Pending {
                    account_id: invited_account_id,
                    principal_id,
                    kind: ugoite_identity::node_identity::InvitationAcceptanceKind::PasskeyRegistration,
                    claimed_at: chrono::Utc::now().to_rfc3339(),
                },
            ),
            created_by: owner_account_id,
        };
        let authorizer = Authorizer::new(state.service.operator().clone());
        authorizer
            .add_human_member(
                &space_uid.to_string(),
                owner_principal_id,
                SpacePrincipal {
                    principal_id,
                    kind: PrincipalKind::Human,
                    display_name: account.display_name.clone(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        authorizer
            .add_human_member(
                &space_uid.to_string(),
                owner_principal_id,
                SpacePrincipal {
                    principal_id: backup_owner_principal_id,
                    kind: PrincipalKind::Human,
                    display_name: "Backup owner".to_string(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Owner,
            )
            .await?;
        authorizer
            .change_role(
                &space_uid.to_string(),
                owner_principal_id,
                owner_principal_id,
                SpaceRole::Viewer,
            )
            .await?;

        bind_invited_account(&state, &account, &invitation, BindingMethod::Invite)
            .await
            .expect("membership-first finalization");
        authorizer
            .revoke_principal(
                &space_uid.to_string(),
                backup_owner_principal_id,
                owner_principal_id,
            )
            .await?;
        bind_invited_account(&state, &account, &invitation, BindingMethod::Invite)
            .await
            .expect("idempotent retry after inviter revocation");

        let authorization = authorizer.state(&space_uid.to_string()).await?;
        assert_eq!(authorization.memberships.len(), 3);
        let node = state.identity.read_state().await?;
        assert_eq!(
            node.bindings
                .iter()
                .filter(|binding| binding.space_uid == space_uid)
                .count(),
            2
        );

        authorizer
            .revoke_principal(
                &space_uid.to_string(),
                backup_owner_principal_id,
                principal_id,
            )
            .await?;
        assert!(
            bind_invited_account(&state, &account, &invitation, BindingMethod::Invite)
                .await
                .is_err()
        );
        Ok(())
    }
}
