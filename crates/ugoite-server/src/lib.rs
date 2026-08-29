#![recursion_limit = "256"]

//! Thin HTTP and MCP adapters over `ugoite-core`.

mod mcp;

use anyhow::Context as _;
use axum::{
    body::{Body, Bytes, HttpBody as _},
    extract::{
        rejection::JsonRejection, DefaultBodyLimit, Extension, Form, Multipart, OriginalUri, Path,
        Query, Request, State,
    },
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{any, delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt};
use http_body::Frame;
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    future::Future,
    net::IpAddr,
    pin::Pin,
    time::Duration,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use ugoite_core::error::{AppError, ErrorCode, ErrorKind};
use ugoite_domain::id::{validate_decoded_space_id, validate_identifier, IdentifierKind};
use ugoite_domain::identity::{
    AccessPolicy, AccountStatus, Action, Actor, AgentMode, AssuranceLevel, AuthenticatedSubject,
    BindingMethod, CredentialConstraints, HumanAccount, NodeRole, PrincipalKind, PrincipalState,
    RequestAuthenticationMethod, RequestIdentity, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::{
    audit::{self, AuditListOptions},
    authorization::{
        AuthorizationState, Authorizer, HumanApproval, HumanApprovalIssue, ResourceKind,
        ResourceRef,
    },
    form, saved_sql,
    service::{
        ApplyOperation, SpacePermission, UgoiteService, MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS,
    },
    space,
};
use ugoite_identity::{
    node_identity::{
        AccountInvitation, ActiveCredentialKind, NodeAuditInput, NodeIdentityService,
        OidcAttemptPurpose, OwnerRecoveryContext, RecoveryBindingSnapshot,
        TotpEnrollmentFinishError,
    },
    oauth::{self, AccessTokenClaims, Confirmation},
};

#[derive(Clone, Copy, Default)]
struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let request_id = Uuid::now_v7()
            .to_string()
            .parse()
            .expect("valid UUID header");
        Some(RequestId::new(request_id))
    }
}
use uuid::Uuid;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

pub const OPENAPI_JSON: &str = include_str!("openapi.json");
const OAUTH_RESOURCE_DOCUMENTATION_URL: &str =
    "https://ugoite.github.io/ugoite/docs/guide/operate/auth/auth-overview/";
const SECURITY_HEADERS_CSP: &str = "default-src 'self'; base-uri 'self'; object-src 'none'; frame-ancestors 'none'; form-action 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self' blob: data:; frame-src 'self' blob:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; worker-src 'self' blob:; manifest-src 'self'";
const HSTS_VALUE: &str = "max-age=31536000; includeSubDomains";
const MAX_SIGNED_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STARTUP_REFRESH_REARM_RETRIES: usize = 8;
const RESPONSE_KEY_ID_HEADER: HeaderName = HeaderName::from_static("x-ugoite-key-id");
const RESPONSE_SIGNATURE_HEADER: HeaderName = HeaderName::from_static("x-ugoite-signature");
const OIDC_STATE_COOKIE: &str = "ugoite_oidc_state";

#[derive(Clone, Debug)]
struct SignableResponseBody(Bytes);

#[derive(Clone, Copy, Debug)]
struct UnsignedResponse;

#[derive(Debug, Eq, PartialEq)]
enum ResponseSigningScope {
    Default,
    Space(String),
    Unsigned,
}

fn response_signing_scope(uri: &Uri) -> ResponseSigningScope {
    let path = uri.path();
    let path = path
        .strip_prefix("/api/")
        .or_else(|| path.strip_prefix('/'))
        .unwrap_or(path);
    let raw_space_id = path
        .strip_prefix("spaces/")
        .map(|rest| rest.split('/').next().unwrap_or(""));
    let Some(raw_space_id) = raw_space_id else {
        return ResponseSigningScope::Default;
    };
    if raw_space_id.is_empty() {
        return ResponseSigningScope::Unsigned;
    }
    let Ok(decoded_space_id) = percent_encoding::percent_decode_str(raw_space_id).decode_utf8()
    else {
        return ResponseSigningScope::Unsigned;
    };
    if validate_decoded_space_id(&decoded_space_id).is_err() {
        return ResponseSigningScope::Unsigned;
    }
    ResponseSigningScope::Space(decoded_space_id.into_owned())
}

fn signable_response_body_size(response: &Response) -> Option<usize> {
    if response.extensions().get::<UnsignedResponse>().is_some()
        || response.headers().contains_key(header::TRAILER)
    {
        return None;
    }
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("text/event-stream")
            })
        })
    {
        return None;
    }
    // This size check is only a bounded materialization gate. The
    // SignableResponseBody contract is attached only after materialization
    // succeeds in mark_signable_api_response below; a fallible body therefore
    // remains unmarked and is replayed unchanged on failure.
    response
        .body()
        .size_hint()
        .exact()
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size <= MAX_SIGNED_RESPONSE_BYTES)
}

fn replay_failed_response_body(
    mut frames: Vec<Result<Frame<Bytes>, axum::Error>>,
    error: Option<axum::Error>,
) -> Body {
    if let Some(error) = error {
        frames.push(Err(error));
    }
    Body::new(http_body_util::StreamBody::new(stream::iter(frames)))
}

async fn collect_response_body_preserving_failure(body: Body, limit: usize) -> Result<Bytes, Body> {
    let mut body_stream = http_body_util::BodyStream::new(body);
    let mut frames = Vec::new();
    let mut chunks = Vec::new();
    let mut size = 0usize;
    while let Some(frame) = body_stream.next().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => return Err(replay_failed_response_body(frames, Some(error))),
        };
        let frame = match frame.into_data() {
            Ok(chunk) => {
                let Some(next_size) = size.checked_add(chunk.len()) else {
                    return Err(replay_failed_response_body(
                        frames,
                        Some(axum::Error::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "response body exceeded signing limit",
                        ))),
                    ));
                };
                if next_size > limit {
                    return Err(replay_failed_response_body(
                        frames,
                        Some(axum::Error::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "response body exceeded signing limit",
                        ))),
                    ));
                }
                size = next_size;
                frames.push(Ok(Frame::data(chunk.clone())));
                chunks.push(chunk);
                continue;
            }
            Err(frame) => frame,
        };
        if let Ok(trailers) = frame.into_trailers() {
            frames.push(Ok(Frame::trailers(trailers)));
            return Err(replay_failed_response_body(frames, None));
        }
        // HTTP body frames are data or trailers; retain an unexpected frame as
        // an unsigned response rather than silently dropping it.
        return Err(replay_failed_response_body(frames, None));
    }
    let mut bytes = Vec::with_capacity(size);
    for chunk in chunks {
        bytes.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(bytes))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SecurityHeadersPolicy {
    hsts: bool,
}

#[cfg(test)]
mod remote_asset_upload_tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post};
    use tower::ServiceExt;

    fn test_identity(space_uid: Uuid, principal_id: Uuid) -> RequestIdentityContext {
        RequestIdentityContext {
            request_identity: RequestIdentity {
                subject: AuthenticatedSubject::HumanAccount {
                    account_id: principal_id,
                },
                actor: Actor::Human {
                    account_id: principal_id,
                },
                credential_id: Uuid::now_v7(),
                authentication_method: RequestAuthenticationMethod::DeviceProof,
                assurance: AssuranceLevel::Possession,
                constraints: CredentialConstraints::default(),
                session_id: None,
            },
            account_id: principal_id,
            display_name: "Asset upload test".to_string(),
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
            recent_passkey: false,
            credential_generation: 0,
            session_token: None,
            human_approval_token: None,
            human_approval_header_invalid: false,
            request_id: Uuid::now_v7(),
        }
    }

    #[tokio::test]
    async fn asset_upload_app_layer_rejects_payloads_over_asset_limit() {
        let state =
            AppState::new_for_tests("memory://server-asset-upload-limit").expect("test state");
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("remote-space", principal_id, "Asset upload test")
            .await
            .expect("create test Space");
        let boundary = "asset-upload-limit-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"large.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend(std::iter::repeat_n(
            b'x',
            ugoite_iceberg::asset::MAX_ASSET_BYTES + 1,
        ));
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let router = Router::new()
            .route("/spaces/{space_id}/assets", post(upload_asset))
            .layer(Extension(test_identity(space_uid, principal_id)));
        let response = app_layers(router, state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/spaces/{space_uid}/assets"))
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn asset_upload_accepts_exact_asset_limit_with_multipart_framing() {
        let state = AppState::new_for_tests("memory://server-asset-upload-exact-limit")
            .expect("test state");
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("remote-space", principal_id, "Asset upload test")
            .await
            .expect("create test Space");
        let boundary = "asset-upload-exact-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"exact.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend(std::iter::repeat_n(
            b'x',
            ugoite_iceberg::asset::MAX_ASSET_BYTES,
        ));
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        assert_eq!(
            upload_status(state, space_uid, principal_id, boundary, body).await,
            StatusCode::CREATED
        );
    }

    async fn upload_status(
        state: AppState,
        space_uid: Uuid,
        principal_id: Uuid,
        boundary: &str,
        body: Vec<u8>,
    ) -> StatusCode {
        let router = Router::new()
            .route("/spaces/{space_id}/assets", post(upload_asset))
            .layer(Extension(test_identity(space_uid, principal_id)));
        app_layers(router, state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/spaces/{space_uid}/assets"))
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response")
            .status()
    }

    fn multipart_body(boundary: &str, fields: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, content) in fields {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"asset.txt\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(content.as_bytes());
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        body
    }

    #[tokio::test]
    async fn asset_upload_rejects_wrong_and_additional_multipart_fields() {
        let state =
            AppState::new_for_tests("memory://server-asset-upload-fields").expect("test state");
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("remote-space", principal_id, "Asset upload test")
            .await
            .expect("create test Space");

        let wrong_field = upload_status(
            state.clone(),
            space_uid,
            principal_id,
            "wrong-field-boundary",
            multipart_body("wrong-field-boundary", &[("other", "content")]),
        )
        .await;
        assert_eq!(wrong_field, StatusCode::BAD_REQUEST);

        let additional_field = upload_status(
            state,
            space_uid,
            principal_id,
            "additional-field-boundary",
            multipart_body(
                "additional-field-boundary",
                &[("file", "content"), ("extra", "content")],
            ),
        )
        .await;
        assert_eq!(additional_field, StatusCode::BAD_REQUEST);
    }
}

impl SecurityHeadersPolicy {
    fn from_public_origin(public_origin: &str) -> Self {
        let hsts = url::Url::parse(public_origin).is_ok_and(|origin| {
            origin.scheme() == "https"
                && !origin.host_str().is_some_and(|host| {
                    let host = host.trim_start_matches('[').trim_end_matches(']');
                    host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
                        || host
                            .parse::<IpAddr>()
                            .is_ok_and(|address| address.is_loopback())
                })
        });
        Self { hsts }
    }

    fn apply(self, headers: &mut HeaderMap) {
        headers.insert(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(SECURITY_HEADERS_CSP),
        );
        headers.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        );
        headers.insert(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        );
        headers.insert(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        );
        if self.hsts {
            headers.insert(
                HeaderName::from_static("strict-transport-security"),
                HeaderValue::from_static(HSTS_VALUE),
            );
        } else {
            headers.remove("strict-transport-security");
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    service: UgoiteService,
    identity: NodeIdentityService,
    security_headers: SecurityHeadersPolicy,
}

impl AppState {
    pub fn new(root_uri: impl Into<String>) -> anyhow::Result<Self> {
        let root_uri = root_uri.into();
        let endpoint = env::var("UGOITE_STORAGE_ENDPOINT").ok();
        let endpoint = space::validate_storage_endpoint(endpoint.as_deref())?;
        let service = UgoiteService::new_with_endpoint(root_uri, endpoint)?;
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
        let identity = NodeIdentityService::new(control_operator, rp_id, public_origin)?;
        Ok(Self {
            security_headers: SecurityHeadersPolicy::from_public_origin(identity.public_origin()),
            identity,
            service,
        })
    }

    #[doc(hidden)]
    pub fn new_for_tests(root_uri: impl Into<String>) -> anyhow::Result<Self> {
        let service = UgoiteService::new(root_uri.into())?;
        Ok(Self {
            identity: NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?,
            security_headers: SecurityHeadersPolicy::from_public_origin("http://localhost:8000"),
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
        let authorizer = Authorizer::new(self.service.operator().clone());
        if let Err(error) = authorizer.ensure_authoritative_mutation_contract() {
            if error
                .downcast_ref::<AppError>()
                .is_some_and(|error| error.code() == ErrorCode::StorageMutationUnavailable)
            {
                // Remote operators are a read-only server mode in v1. Do not
                // bootstrap identity, recover claims, or start maintenance:
                // those paths mutate multiple authoritative objects. Existing
                // response-signing material remains usable by request paths.
                return Ok(());
            }
            return Err(error);
        }
        if let Err(error) = authorizer.verify_authoritative_storage("startup").await {
            if error
                .downcast_ref::<AppError>()
                .is_some_and(|error| error.code() == ErrorCode::StorageMutationUnavailable)
            {
                // Capability bits alone do not establish a usable shared
                // mutation contract. Stay read-only until the active probe
                // succeeds on a later startup.
                return Ok(());
            }
            return Err(error);
        }
        if let Some(bootstrap) = self.identity.bootstrap_if_needed().await? {
            println!(
                "Ugoite setup URL (expires {}): {}",
                bootstrap.expires_at, bootstrap.setup_url
            );
        }
        // Claim-backed Space creation is the explicit recovery boundary. Run
        // it before strict enumeration so a crash-left pending bootstrap does
        // not prevent the server from reaching its listener on restart.
        self.service.recover_pending_space_claims().await?;
        let space_ids = self.service.list_space_ids().await?;
        // Resolve every durable recovery fence and audit obligation before
        // launching maintenance. Maintenance can mutate derived/asset
        // storage, so it must not race an unresolved recovery decision.
        for space_id in &space_ids {
            reconcile_recovery_fences(self, space_id).await?;
            reconcile_recovery_audit_outbox(self, space_id).await?;
            reconcile_human_approval_audit_outbox(self, space_id).await?;
        }
        for space_id in space_ids {
            // Rehydrate relation-local maintenance on every server start.
            let maintenance_service = self.service.clone();
            let maintenance_space_id = space_id.clone();
            tokio::spawn(async move {
                let _ = maintenance_service
                    .rearm_asset_text_gc(&maintenance_space_id)
                    .await;
                for attempt in 0..=MAX_STARTUP_REFRESH_REARM_RETRIES {
                    match maintenance_service
                        .rearm_asset_text_refresh(&maintenance_space_id)
                        .await
                    {
                        Ok(()) => return,
                        Err(error) => {
                            eprintln!(
                                "AssetText startup refresh rearm failed for Space {} (attempt {}): {error:#}{}",
                                maintenance_space_id,
                                attempt + 1,
                                if attempt < MAX_STARTUP_REFRESH_REARM_RETRIES {
                                    "; retrying"
                                } else {
                                    "; durable stale state remains for explicit repair"
                                },
                            );
                            if attempt < MAX_STARTUP_REFRESH_REARM_RETRIES {
                                let delay = Duration::from_secs(1u64 << (attempt + 1).min(6));
                                tokio::time::sleep(delay).await;
                            }
                        }
                    }
                }
            });
        }
        Ok(())
    }
}

async fn reconcile_human_approval_audit_outbox(
    state: &AppState,
    space_id: &str,
) -> anyhow::Result<()> {
    let authorizer = Authorizer::new(state.service.operator().clone());
    for record in authorizer.pending_human_approval_audits(space_id).await? {
        audit::append_audit_event(state.service.operator(), space_id, &record.event, None).await?;
        authorizer
            .mark_human_approval_audit_delivered(space_id, record.event_id)
            .await?;
    }
    Ok(())
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
    session_token: Option<String>,
    human_approval_token: Option<String>,
    human_approval_header_invalid: bool,
    request_id: Uuid,
}

fn publication_command_id(
    headers: &HeaderMap,
    operation: &str,
    fallback_request_id: Uuid,
) -> ApiResult<String> {
    let key = headers
        .get("idempotency-key")
        .map(|value| {
            value
                .to_str()
                .map(|value| value.trim().to_owned())
                .map_err(|_| {
                    ApiError::new(StatusCode::BAD_REQUEST, "Idempotency-Key must be ASCII")
                })
        })
        .transpose()?
        .unwrap_or_else(|| fallback_request_id.to_string());
    if key.is_empty() || key.len() > 256 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Idempotency-Key must contain 1 to 256 characters",
        ));
    }
    Ok(format!(
        "{operation}-{}",
        hex::encode(Sha256::digest(key.as_bytes()))
    ))
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
        .route(
            "/auth/oidc/providers/{provider_id}",
            delete(disable_oidc_provider),
        )
        .route("/auth/oidc/links", get(list_oidc_links))
        .route("/auth/oidc/links/{method_id}", delete(unlink_oidc))
        .route("/auth/passkeys", get(list_passkeys))
        .route("/auth/sessions", get(list_sessions))
        .route("/auth/sessions/{session_id}", delete(revoke_session_by_id))
        .route("/auth/passkeys/start", post(start_add_passkey))
        .route("/auth/passkeys/finish", post(finish_add_passkey))
        .route(
            "/auth/passkeys/bootstrap/start",
            post(start_bootstrap_passkey),
        )
        .route(
            "/auth/passkeys/bootstrap/finish",
            post(finish_bootstrap_passkey),
        )
        .route("/auth/passkeys/{credential_id}", delete(revoke_passkey))
        .route("/auth/recovery/totp/start", post(start_totp_enrollment))
        .route("/auth/recovery/totp/finish", post(finish_totp_enrollment))
        .route(
            "/spaces/{space_id}/admin/recovery/force-reset",
            post(owner_force_reset),
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
        .route("/spaces/{space_id}/approvals", post(issue_human_approval))
        .route("/spaces", get(list_spaces).post(create_space))
        .route("/spaces/{space_id}", get(get_space).patch(patch_space))
        .route("/spaces/{space_id}/health", get(space_health))
        .route("/spaces/{space_id}/pins/diff", get(pin_diff))
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
        .route("/spaces/{space_id}/pins", get(list_pins).post(create_pin))
        .route("/spaces/{space_id}/pins/{pin_name}", delete(delete_pin))
        .route("/spaces/{space_id}/changes", get(list_changes))
        .route(
            "/spaces/{space_id}/changes/{change_id}/revert",
            post(revert_change),
        )
        .route("/spaces/{space_id}/runs/{run_id}/undo", post(undo_run))
        .route("/spaces/{space_id}/apply", post(apply_operations))
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
        .layer(middleware::from_fn(mark_signable_api_response))
}

async fn mark_signable_api_response(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let Some(exact_size) = signable_response_body_size(&response) else {
        return response;
    };
    let (parts, body) = response.into_parts();
    match collect_response_body_preserving_failure(body, exact_size).await {
        Ok(bytes) => {
            let mut response = Response::from_parts(parts, Body::from(bytes.clone()));
            response
                .extensions_mut()
                .insert(SignableResponseBody(bytes));
            response
        }
        Err(body) => Response::from_parts(parts, body),
    }
}

fn app_layers(router: Router<AppState>, state: AppState) -> Router {
    let mut router = router
        .layer(DefaultBodyLimit::max(
            ugoite_iceberg::asset::MAX_ASSET_BYTES
                + ugoite_iceberg::asset::MAX_ASSET_MULTIPART_OVERHEAD_BYTES,
        ))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuidV7))
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
                    .expose_headers([RESPONSE_KEY_ID_HEADER, RESPONSE_SIGNATURE_HEADER])
                    .allow_headers([
                        header::ACCEPT,
                        header::AUTHORIZATION,
                        header::CONTENT_TYPE,
                        HeaderName::from_static("idempotency-key"),
                        HeaderName::from_static("dpop"),
                        HeaderName::from_static("x-request-id"),
                        HeaderName::from_static("x-ugoite-human-approval"),
                        HeaderName::from_static("mcp-method"),
                        HeaderName::from_static("mcp-name"),
                        HeaderName::from_static("mcp-protocol-version"),
                    ]),
            );
        }
    }
    router = router.layer(middleware::from_fn_with_state(
        state.clone(),
        add_security_headers,
    ));
    router.with_state(state)
}

async fn add_security_headers(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let no_store = request.uri().path().contains("/approvals")
        || (matches!(*request.method(), Method::DELETE | Method::PUT)
            && (request.uri().path().contains("/entries/")
                || request.uri().path().contains("/sql/")
                || request.uri().path().contains("/assets/")
                || request.uri().path().contains("/policies/")));
    let uri = request
        .extensions()
        .get::<OriginalUri>()
        .map(|OriginalUri(uri)| uri.clone())
        .unwrap_or_else(|| request.uri().clone());
    let is_head = request.method() == Method::HEAD;
    let scope = response_signing_scope(&uri);
    let mut response = next.run(request).await;
    state.security_headers.apply(response.headers_mut());
    if no_store {
        response.headers_mut().insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::from_static("no-store"),
        );
    }
    if scope == ResponseSigningScope::Unsigned {
        return response;
    }
    let Some(SignableResponseBody(materialized_bytes)) =
        response.extensions().get::<SignableResponseBody>().cloned()
    else {
        return response;
    };
    let bytes = if is_head {
        Bytes::new()
    } else {
        materialized_bytes
    };
    let (parts, _body) = response.into_parts();
    let mut response = Response::from_parts(parts, Body::from(bytes.clone()));
    let signing = match scope {
        ResponseSigningScope::Default => {
            ugoite_iceberg::integrity::build_default_response_signature(
                state.service.operator(),
                &bytes,
            )
            .await
        }
        ResponseSigningScope::Space(space_id) => {
            ugoite_iceberg::integrity::build_response_signature(
                state.service.operator(),
                &space_id,
                &bytes,
            )
            .await
        }
        ResponseSigningScope::Unsigned => return response,
    };
    let Ok((key_id, signature)) = signing else {
        return response;
    };
    let (Ok(key_id), Ok(signature)) = (
        HeaderValue::from_str(&key_id),
        HeaderValue::from_str(&signature),
    ) else {
        return response;
    };
    response
        .headers_mut()
        .insert(RESPONSE_KEY_ID_HEADER, key_id);
    response
        .headers_mut()
        .insert(RESPONSE_SIGNATURE_HEADER, signature);
    response
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
            .route("/mcp", any(mcp::handle))
            .route_service("/", ServeFile::new(format!("{static_dir}/index.html")))
            .nest("/api", api_routes(state.clone()))
            .fallback_service(
                ServeDir::new(&static_dir)
                    .fallback(ServeFile::new(format!("{static_dir}/index.html"))),
            )
    } else {
        metadata
            .merge(api_routes(state.clone()))
            .route("/mcp", any(mcp::handle))
            .route(
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
    // Client request IDs remain useful for transport tracing, but approval
    // audit event IDs must be server-owned so a caller cannot deliberately
    // replay an audit id and suppress a denial event.
    let request_id = Uuid::now_v7();
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        request.headers_mut().insert("x-request-id", value);
    }
    let human_approval_header_invalid = headers
        .get("x-ugoite-human-approval")
        .is_some_and(|value| value.to_str().is_err());
    let human_approval_token = headers
        .get("x-ugoite-human-approval")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let path = request
        .uri()
        .path()
        .strip_prefix("/api")
        .unwrap_or(request.uri().path());
    let setup_strengthening = path == "/auth/session" || path.starts_with("/auth/passkeys");
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
                session_token: Some(session_id),
                human_approval_token: human_approval_token.clone(),
                human_approval_header_invalid,
                request_id,
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
        // RFC 9449 `htu` is the scheme/authority/path only. Query parameters
        // are request data, not part of the DPoP URI binding.
        let htu = format!("{}{}", issuer.trim_end_matches('/'), request.uri().path());
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
            session_token: None,
            human_approval_token,
            human_approval_header_invalid,
            request_id,
        }
    };
    request.extensions_mut().insert(identity);
    next.run(request).await
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "code": "AUTHENTICATION_REQUIRED",
            "message": message,
        })),
    )
        .into_response()
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
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
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
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
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
    // Setup spans Node identity and every existing Space. Shared object
    // storage cannot commit those objects as one transaction, so reject the
    // whole bootstrap before consuming the one-time setup secret.
    for space_id in &existing_spaces {
        let space_uid = state
            .service
            .space_uid(space_id)
            .await
            .map_err(ApiError::from_core)?;
        space::validate_complete_bootstrap(state.service.operator(), space_id)
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
    let mut claims = Vec::new();
    if existing_spaces.is_empty() {
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("default", principal_id, &result.account.display_name)
            .await
            .map_err(ApiError::from_core)?;
        claims.push((space_uid, principal_id));
    } else {
        for space_id in &existing_spaces {
            let space_uid = state
                .service
                .space_uid(space_id)
                .await
                .map_err(ApiError::from_core)?;
            let principal_id = authorizer
                .ensure_owner(space_id, space_uid, &result.account.display_name)
                .await
                .map_err(ApiError::from_core)?;
            claims.push((space_uid, principal_id));
        }
    }
    state
        .identity
        .add_bindings(
            claims
                .iter()
                .map(
                    |(space_uid, principal_id)| ugoite_domain::identity::PrincipalBinding {
                        space_uid: *space_uid,
                        principal_id: *principal_id,
                        node_account_id: result.account.account_id,
                        binding_method: BindingMethod::Setup,
                    },
                )
                .collect(),
        )
        .await
        .map_err(auth_error)?;
    let claimed_space_uids = claims
        .iter()
        .map(|(space_uid, _)| *space_uid)
        .collect::<Vec<_>>();
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
    reconcile_all_recovery_fences_api(&state).await?;
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
    reconcile_all_recovery_fences_api(&state).await?;
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
    reconcile_all_recovery_fences_api(&state).await?;
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
        [
            (
                "set-cookie",
                auth_cookie(&result.session_id, 60 * 60 * 24 * 30),
            ),
            ("cache-control", "no-store".to_string()),
        ],
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
    let session_token = identity.session_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "owner recovery requires a browser Passkey session"
            }),
        )
    })?;
    state
        .identity
        .revalidate_recent_passkey_session(
            session_token,
            identity.account_id,
            identity.request_identity.credential_id,
            identity.credential_generation,
        )
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::FORBIDDEN,
                json!({
                    "code": "RECOVERY_AUTHORITY_REQUIRED",
                    "message": "the Owner Passkey session is no longer valid"
                }),
            )
        })?;
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
    if matching.len() != 1 {
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
    let space_id = find_space_id_by_uid(state, context.space_uid).await?;
    let authorization = Authorizer::new(state.service.operator().clone())
        .state(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let fence = context
        .recovery_fence_id
        .and_then(|fence_id| authorization.recovery_fences.get(&fence_id))
        .ok_or_else(recovery_fence_unavailable)?;
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
#[allow(clippy::too_many_arguments)]
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
    let authorizer = Authorizer::new(state.service.operator().clone());

    // A Space CAS can commit while its response is lost, either before or
    // after the Node fence is promoted from provisional to paired. Reattach
    // the request-identified Node fence before attempting a new reservation;
    // otherwise the active Node half would make every same-key retry fail
    // closed until expiry.
    if let Some(provisional) = state
        .identity
        .recovery_fence_for_request(
            request_id,
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
        )
        .await
        .map_err(recovery_commit_error)?
    {
        let authorization = authorizer
            .state(space_id)
            .await
            .map_err(|_| recovery_fence_unavailable())?;
        let fence = match authorization
            .recovery_fences
            .get(&provisional.recovery_fence_id)
            .cloned()
        {
            Some(fence) => fence,
            None => {
                // A read which misses the Space fence is not proof that a
                // concurrent CAS is not in flight. Retry the same Space CAS
                // with the same fence identity; the Authorizer treats an
                // exact active identity as idempotent and a storage outcome
                // that remains ambiguous keeps the Node half fail-closed.
                authorizer
                    .reserve_recovery_fence_with_id(
                        space_id,
                        request_id,
                        provisional.recovery_fence_id,
                        issuer_principal_id,
                        issuer_account_id,
                        target_principal_id,
                        target_account_id,
                        issuer_generation,
                        target_generation,
                        ttl,
                    )
                    .await
                    .map_err(|_| recovery_fence_unavailable())?
            }
        };
        if fence.status != "active"
            || fence.request_id != request_id
            || fence.space_uid != space_uid
            || fence.issuer_principal_id != issuer_principal_id
            || fence.issuer_account_id != issuer_account_id
            || fence.target_principal_id != target_principal_id
            || fence.target_account_id != target_account_id
            || fence.issuer_generation != issuer_generation
            || fence.target_generation != target_generation
            || !chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                .map(|expires| expires.with_timezone(&chrono::Utc) > chrono::Utc::now())
                .unwrap_or(false)
            || !chrono::DateTime::parse_from_rfc3339(&provisional.recovery_fence_expires_at)
                .map(|expires| expires.with_timezone(&chrono::Utc) > chrono::Utc::now())
                .unwrap_or(false)
        {
            return Err(recovery_fence_unavailable());
        }
        let snapshot = RecoveryBindingSnapshot {
            request_id,
            recovery_fence_id: fence.fence_id,
            recovery_fence_expires_at: fence.expires_at.clone(),
            space_authorization_revision: fence.authorization_revision,
            issuer_space_lifecycle_epoch: fence.issuer_space_lifecycle_epoch,
            target_space_lifecycle_epoch: fence.target_space_lifecycle_epoch,
            issuer_node_lifecycle_epoch: provisional.issuer_node_lifecycle_epoch,
            target_node_lifecycle_epoch: provisional.target_node_lifecycle_epoch,
            issuer_generation,
            target_generation,
        };
        state
            .identity
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&snapshot),
            )
            .await
            .map_err(recovery_commit_error)?;
        return Ok((authorizer, fence, snapshot));
    }
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
            if space_write_outcome_is_ambiguous(&error) {
                // The Space fence may already be durable. Keep the Node half
                // until reconciliation can inspect the paired outcome.
                return Err(recovery_fence_unavailable());
            }
            state
                .identity
                .release_recovery_fence(fence_id)
                .await
                .map_err(recovery_commit_error)?;
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
        state
            .identity
            .release_recovery_fence(fence.fence_id)
            .await
            .map_err(recovery_commit_error)?;
        authorizer
            .release_recovery_fence(space_id, fence.fence_id)
            .await
            .map_err(|_| recovery_storage_unavailable())?;
        return Err(recovery_commit_error(error));
    }
    Ok((authorizer, fence, snapshot))
}

/// Reserve only the Space half for a replacement approval. The previous
/// approval may own a Node fence in another Space; the Node CAS below replaces
/// that fence and inserts the new one atomically. If either pre-commit step
/// fails, the old approval remains durable and usable.
#[allow(clippy::too_many_arguments)]
async fn reserve_recovery_space_fence(
    state: &AppState,
    space_id: &str,
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
    let authorizer = Authorizer::new(state.service.operator().clone());
    let fence = authorizer
        .reserve_recovery_fence_with_id(
            space_id,
            request_id,
            Uuid::now_v7(),
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            issuer_generation,
            target_generation,
            ttl,
        )
        .await
        .map_err(|error| {
            if space_write_outcome_is_ambiguous(&error) {
                recovery_fence_unavailable()
            } else {
                recovery_reservation_error(error)
            }
        })?;
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
    Ok((authorizer, fence, snapshot))
}

async fn ensure_owner_recovery_fence(
    context: &OwnerRecoveryContext,
) -> ApiResult<OwnerRecoveryContext> {
    if context.recovery_fence_id.is_some() {
        return Ok(context.clone());
    }
    // Pre-fence approvals do not carry the original Space lifecycle tuple.
    // Rebinding them to the current tuple would make a role or membership
    // transition indistinguishable from an approval issued afterward.
    Err(owner_recovery_api_error(anyhow::anyhow!(
        "legacy owner approval is stale"
    )))
}

fn recovery_reservation_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    let lower = message.to_lowercase();
    if space_write_outcome_is_ambiguous(&error) {
        return recovery_fence_unavailable();
    }
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

fn recovery_write_outcome_is_ambiguous(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("node control write committed")
        || message.contains("node control write outcome unknown")
}

fn space_write_outcome_is_ambiguous(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("Space authorization write committed")
        || message.contains("Space authorization write outcome unknown")
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
            "active" => {
                let expires_at = chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                    .map(|expires| expires.with_timezone(&chrono::Utc))
                    .map_err(|_| anyhow::anyhow!("invalid stored recovery fence timestamp"))?;
                if expires_at <= chrono::Utc::now() {
                    // The Node mutation is durable, but the Space half never
                    // committed. Release both halves as one terminal
                    // reconciliation; expiry is not a Node-only release.
                    authorizer
                        .release_recovery_fence(space_id, fence_id)
                        .await?;
                    state
                        .identity
                        .abort_recovery_fence_after_space_abort(fence_id)
                        .await?;
                    continue;
                }
                authorizer
                    .complete_recovery_fence(space_id, fence_id)
                    .await
                    .is_ok()
            }
            "completed" => true,
            // A Node mutation is durable once it appears in pending_ids. A
            // released Space fence is nevertheless a safe terminal outcome:
            // recovery changes Node credentials only, never Space membership.
            "released" => {
                state
                    .identity
                    .abort_recovery_fence_after_space_abort(fence_id)
                    .await?;
                continue;
            }
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
            if state
                .identity
                .expired_recovery_fence(fence_id)
                .await
                .unwrap_or(false)
            {
                state
                    .identity
                    .abort_recovery_fence_after_space_abort(fence_id)
                    .await?;
            }
            continue;
        }
        state
            .identity
            .mark_recovery_fence_reconciled(fence_id)
            .await?;
    }

    // A force-reset request has a server-generated request ID, so a caller
    // cannot retry an ambiguous Space CAS by itself. Replay every durable
    // provisional Node reservation during reconciliation; the exact fence and
    // request identities make this safe whether the original Space write
    // committed or not.
    for provisional in state
        .identity
        .active_provisional_recovery_fences(space_uid)
        .await?
    {
        let _ = reserve_recovery_pair(
            state,
            space_id,
            space_uid,
            provisional.issuer_principal_id,
            provisional.issuer_account_id,
            provisional.target_principal_id,
            provisional.target_account_id,
            provisional.snapshot.request_id,
            chrono::Duration::minutes(15),
        )
        .await;
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
        state.identity.release_recovery_fence(fence_id).await?;
        authorizer
            .release_recovery_fence(space_id, fence_id)
            .await?;
    }
    // A supersession may commit the Node-side invalidation before the paired
    // Space release. On restart, the terminal Node status is enough to finish
    // that release; an active Node status remains fail-closed.
    let current_authorization = authorizer.state(space_id).await?;
    for fence in current_authorization
        .recovery_fences
        .values()
        .filter(|fence| fence.status == "active" && !pending_ids.contains(&fence.fence_id))
    {
        if matches!(
            state
                .identity
                .recovery_fence_status(fence.fence_id)
                .await?
                .as_deref(),
            Some("released" | "superseded" | "completed")
        ) {
            authorizer
                .release_recovery_fence(space_id, fence.fence_id)
                .await?;
        }
    }
    for fence_id in state.identity.active_recovery_fence_ids(space_uid).await? {
        if pending_ids.contains(&fence_id) {
            continue;
        }
        let phase = state.identity.recovery_fence_phase(fence_id).await?;
        let Some(space_fence) = current_authorization.recovery_fences.get(&fence_id) else {
            // A read which misses the Space fence cannot prove that a CAS is
            // not in flight in another process. Keep both the provisional and
            // paired Node barriers fail-closed; the same-key retry replays the
            // exact Space fence identity and can converge once that CAS is
            // observable. Releasing here could let a delayed Space write
            // publish a fence with no Node barrier.
            continue;
        };
        if matches!(space_fence.status.as_str(), "released" | "completed") {
            state.identity.release_recovery_fence(fence_id).await?;
            continue;
        }
        if phase.as_deref() != Some("provisional")
            && state.identity.expired_recovery_fence(fence_id).await?
        {
            // Do not release a paired Node fence based on its local clock
            // alone; its Space half must already be terminal.
            continue;
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

async fn reconcile_recovery_fences_api(state: &AppState, space_id: &str) -> ApiResult<()> {
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    Authorizer::new(state.service.operator().clone())
        .verify_authoritative_storage(space_id)
        .await
        .map_err(ApiError::from_core)?;
    if reconcile_recovery_fences(state, space_id).await.is_ok() {
        return Ok(());
    }
    let committed = match state.service.space_uid(space_id).await {
        Ok(space_uid) => state
            .identity
            .pending_recovery_fence_ids(space_uid)
            .await
            .is_ok_and(|fences| !fences.is_empty()),
        Err(_) => false,
    };
    if committed {
        Err(recovery_fence_unavailable())
    } else {
        Err(recovery_storage_unavailable())
    }
}

async fn reconcile_all_recovery_fences_api(state: &AppState) -> ApiResult<()> {
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    Authorizer::new(state.service.operator().clone())
        .verify_authoritative_storage("recovery")
        .await
        .map_err(ApiError::from_core)?;
    if reconcile_all_recovery_fences(state).await.is_ok() {
        return Ok(());
    }
    let mut committed = false;
    if let Ok(space_ids) = state.service.list_space_ids().await {
        for space_id in space_ids {
            let Ok(space_uid) = state.service.space_uid(&space_id).await else {
                continue;
            };
            if state
                .identity
                .pending_recovery_fence_ids(space_uid)
                .await
                .is_ok_and(|fences| !fences.is_empty())
            {
                committed = true;
                break;
            }
        }
    }
    if committed {
        Err(recovery_fence_unavailable())
    } else {
        Err(recovery_storage_unavailable())
    }
}

async fn abort_owner_recovery_fence(
    state: &AppState,
    space_uid: Uuid,
    fence_id: Uuid,
) -> ApiResult<()> {
    let space_id = find_space_id_by_uid(state, space_uid)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    state
        .identity
        .release_recovery_fence(fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Authorizer::new(state.service.operator().clone())
        .release_recovery_fence(&space_id, fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Ok(())
}

fn recovery_result_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
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
                        actor_account_id: record.actor_account_id,
                        credential_id: record.actor_credential_id.or(record.credential_id),
                        action: &record.action,
                        target_type: "space_principal",
                        target_id: Some(record.principal_id.to_string()),
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
    reconcile_recovery_fences_api(&state, &space_id).await?;
    // Force-reset issuance is deliberately not idempotent. A deliberate new
    // approval supersedes the previous one, while the request identity is
    // generated by the server and never supplied by the caller.
    let request_id = Uuid::now_v7();
    let (space_uid, issuer_principal_id) =
        recovery_owner_context(&state, &space_id, &identity).await?;
    let owner_session_token = identity.session_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "owner recovery requires a browser Passkey session"
            }),
        )
    })?;
    let account_id =
        recovery_target_account(&state, &space_id, space_uid, payload.principal_id).await?;
    let existing_fence = state
        .identity
        .active_owner_recovery_fence(account_id)
        .await
        .map_err(recovery_commit_error)?;
    let (authorizer, fence, snapshot, old_space_fence, reuse_existing_fence) =
        if let Some((old_space_uid, old_snapshot)) = existing_fence {
            let old_space_id = find_space_id_by_uid(&state, old_space_uid)
                .await
                .map_err(|_| recovery_storage_unavailable())?;
            let old_authorizer = Authorizer::new(state.service.operator().clone());
            let old_space_state = old_authorizer
                .state(&old_space_id)
                .await
                .map_err(|_| recovery_fence_unavailable())?;
            let old_fence = old_space_state
                .recovery_fences
                .get(&old_snapshot.recovery_fence_id)
                .cloned()
                .ok_or_else(recovery_fence_unavailable)?;
            if old_fence.status != "active"
                || old_fence.space_uid != old_space_uid
                || old_fence.target_principal_id != payload.principal_id
                || old_fence.target_account_id != account_id
            {
                return Err(recovery_fence_unavailable());
            }
            let same_tuple = old_space_uid == space_uid
                && old_fence.issuer_principal_id == issuer_principal_id
                && old_fence.issuer_account_id == identity.account_id;
            if same_tuple {
                (old_authorizer, old_fence, old_snapshot, None, true)
            } else if old_space_uid == space_uid {
                // A different active issuer cannot borrow the existing Space
                // fence. Leave the old approval untouched and fail closed.
                return Err(recovery_fence_unavailable());
            } else {
                let (authorizer, fence, snapshot) = reserve_recovery_space_fence(
                    &state,
                    &space_id,
                    issuer_principal_id,
                    identity.account_id,
                    payload.principal_id,
                    account_id,
                    request_id,
                    chrono::Duration::minutes(15),
                )
                .await?;
                (
                    authorizer,
                    fence,
                    snapshot,
                    Some((old_space_uid, old_snapshot.recovery_fence_id)),
                    false,
                )
            }
        } else {
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
            (authorizer, fence, snapshot, None, false)
        };
    if state
        .identity
        .revalidate_recent_passkey_session(
            owner_session_token,
            identity.account_id,
            identity.request_identity.credential_id,
            identity.credential_generation,
        )
        .await
        .is_err()
    {
        if !reuse_existing_fence {
            state
                .identity
                .release_recovery_fence(fence.fence_id)
                .await
                .map_err(recovery_commit_error)?;
            authorizer
                .release_recovery_fence(&space_id, fence.fence_id)
                .await
                .map_err(|_| recovery_storage_unavailable())?;
        }
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            json!({
                "code": "RECOVERY_AUTHORITY_REQUIRED",
                "message": "the Owner Passkey session is no longer valid"
            }),
        ));
    }
    let issued = if reuse_existing_fence {
        state
            .identity
            .replace_owner_recovery_approval_with_snapshot_credential_and_session(
                space_uid,
                payload.principal_id,
                account_id,
                issuer_principal_id,
                identity.account_id,
                snapshot,
                Some(identity.request_identity.credential_id),
                owner_session_token,
                request_id,
                None,
            )
            .await
    } else if let Some((_, old_fence_id)) = old_space_fence {
        state
            .identity
            .replace_owner_recovery_approval_with_snapshot_credential_and_session(
                space_uid,
                payload.principal_id,
                account_id,
                issuer_principal_id,
                identity.account_id,
                snapshot,
                Some(identity.request_identity.credential_id),
                owner_session_token,
                request_id,
                Some(old_fence_id),
            )
            .await
    } else {
        state
            .identity
            .issue_owner_recovery_approval_with_snapshot_credential_and_session(
                space_uid,
                payload.principal_id,
                account_id,
                issuer_principal_id,
                identity.account_id,
                snapshot,
                Some(identity.request_identity.credential_id),
                owner_session_token,
            )
            .await
    };
    let (_, token, expires_at) = match issued {
        Ok(result) => result,
        Err(error) => {
            if recovery_write_outcome_is_ambiguous(&error) {
                // The Node approval may already be durable. Keep both fences
                // active so reconciliation can discover the approval instead
                // of releasing a committed mutation with no replayable token.
                return Err(recovery_fence_unavailable());
            }
            if !reuse_existing_fence {
                state
                    .identity
                    .release_recovery_fence(fence.fence_id)
                    .await
                    .map_err(recovery_commit_error)?;
                authorizer
                    .release_recovery_fence(&space_id, fence.fence_id)
                    .await
                    .map_err(|_| recovery_storage_unavailable())?;
            }
            return Err(recovery_commit_error(error));
        }
    };
    if let Some((old_space_uid, old_fence_id)) = old_space_fence {
        if abort_owner_recovery_fence(&state, old_space_uid, old_fence_id)
            .await
            .is_err()
        {
            // The Node CAS already made the replacement authoritative. Keep
            // the new result fail-closed until reconciliation releases the
            // old Space half instead of manufacturing a clean success.
            return Err(recovery_fence_unavailable());
        }
    }
    let mut headers = recovery_result_headers();
    Ok((
        StatusCode::CREATED,
        std::mem::take(&mut headers),
        Json(json!({
            "principal_id": payload.principal_id,
            "owner_approval_token": token,
            "expires_at": expires_at,
            "audit_status": "pending"
        })),
    )
        .into_response())
}

async fn auth_owner_recovery_start(
    State(state): State<AppState>,
    Json(payload): Json<OwnerRecoveryStartRequest>,
) -> ApiResult<Response> {
    reconcile_all_recovery_fences_api(&state).await?;
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
    let context = ensure_owner_recovery_fence(&context).await?;
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
    reconcile_all_recovery_fences_api(&state).await?;
    match state
        .identity
        .take_owner_recovery_response_for_challenge(payload.challenge_id, &payload.credential)
        .await
    {
        Ok(Some((account, session_token, recovery_codes, marker))) => {
            if let Ok(space_id) = find_space_id_by_uid(&state, marker.space_uid).await {
                let _ = reconcile_recovery_audit_outbox(&state, &space_id).await;
            }
            let audit_delivered = state
                .identity
                .pending_recovery_audits()
                .await
                .map(|pending| {
                    !pending
                        .into_iter()
                        .any(|record| record.event_id == marker.reset_id)
                })
                .unwrap_or(false);
            let mut headers = recovery_result_headers();
            headers.insert(
                header::SET_COOKIE,
                HeaderValue::from_str(&auth_cookie(&session_token, 60 * 60 * 24 * 30)).map_err(
                    |_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "invalid auth cookie"),
                )?,
            );
            return Ok((
                StatusCode::CREATED,
                headers,
                Json(json!({
                    "account": account,
                    "recovery_codes": recovery_codes,
                    "audit_status": if audit_delivered { "delivered" } else { "pending" }
                })),
            )
                .into_response());
        }
        Ok(None) => {}
        Err(error)
            if error.to_string().contains("already delivered")
                || error.to_string().contains("no longer current") =>
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                json!({"code":"SPACE_RECOVERY_ALREADY_COMPLETED","message":"Space access recovery response is already committed"}),
            ));
        }
        Err(error) => return Err(owner_recovery_commit_api_error(error)),
    }
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
    let context = ensure_owner_recovery_fence(&context).await?;
    validate_owner_recovery_context(&state, &context).await?;
    let result = state
        .identity
        .finish_owner_recovery_registration(payload.challenge_id, &payload.credential)
        .await
        .map_err(owner_recovery_commit_api_error)?;
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
        state
            .identity
            .mark_recovery_fence_reconciled(fence_id)
            .await
            .map_err(|_| recovery_fence_unavailable())?;
    }
    let (_, session_token, recovery_codes, _) = match state
        .identity
        .take_owner_recovery_response_for_challenge(payload.challenge_id, &payload.credential)
        .await
    {
        Ok(Some(response)) => response,
        Ok(None) => return Err(recovery_fence_unavailable()),
        Err(error)
            if error.to_string().contains("already delivered")
                || error.to_string().contains("no longer current") =>
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                json!({"code":"SPACE_RECOVERY_ALREADY_COMPLETED","message":"Space access recovery response is already committed"}),
            ));
        }
        Err(_) => return Err(recovery_fence_unavailable()),
    };
    let mut headers = recovery_result_headers();
    let cookie = auth_cookie(&session_token, 60 * 60 * 24 * 30);
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "invalid auth cookie"))?,
    );
    let recovery_event_id = result.recovery_request_id.unwrap_or(payload.challenge_id);
    if let Ok(space_id) = find_space_id_by_uid(&state, space_uid).await {
        let _ = reconcile_recovery_audit_outbox(&state, &space_id).await;
    }
    let audit_delivered = state
        .identity
        .pending_recovery_audits()
        .await
        .map(|pending| {
            !pending
                .into_iter()
                .any(|record| record.event_id == recovery_event_id)
        })
        .unwrap_or(false);
    Ok((
        StatusCode::CREATED,
        headers,
        Json(json!({
            "account": result.account,
            "recovery_codes": recovery_codes,
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
    } else if message.contains("no longer current") || message.contains("already completed") {
        (StatusCode::CONFLICT, "SPACE_RECOVERY_ALREADY_COMPLETED")
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

fn owner_recovery_commit_api_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("node control write committed")
        || message.contains("node control write outcome unknown")
    {
        return recovery_fence_unavailable();
    }
    owner_recovery_api_error(error)
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

fn request_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == cookie_name).then(|| value.to_string())
            })
        })
}

fn auth_session_cookie(headers: &HeaderMap) -> Option<String> {
    request_cookie(headers, "ugoite_session")
}

fn oidc_state_cookie(headers: &HeaderMap) -> Option<String> {
    request_cookie(headers, OIDC_STATE_COOKIE)
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

fn oidc_state_cookie_header(state_hash: &str, max_age_seconds: i64) -> String {
    let secure = env::var("UGOITE_PUBLIC_ORIGIN")
        .unwrap_or_default()
        .starts_with("https://");
    format!(
        "{OIDC_STATE_COOKIE}={state_hash}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn clear_oidc_state_cookie() -> String {
    oidc_state_cookie_header("", 0)
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

fn access_policy_json_rejection(error: JsonRejection) -> ApiError {
    ApiError::new(
        error.status(),
        json!({
            "code": "INVALID_INPUT",
            "message": error.body_text(),
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

fn redact_oidc_provider_secret(mut value: Value) -> Value {
    if let Value::Object(object) = &mut value {
        object.remove("client_secret");
    }
    value
}

fn normalized_oidc_issuer(issuer: &str) -> anyhow::Result<String> {
    let normalized = issuer.trim().trim_end_matches('/');
    let parsed = url::Url::parse(normalized).context("invalid OIDC issuer URL")?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!("OIDC issuer must use https without userinfo, query, or fragment")
    }
    Ok(normalized.to_string())
}

fn validate_oidc_endpoint(url: &url::Url, name: &str) -> anyhow::Result<()> {
    // The in-process mock issuer used by server tests is intentionally plain
    // HTTP on loopback. Production provider configuration and all non-test
    // endpoints remain HTTPS-only.
    if cfg!(test)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
    {
        return Ok(());
    }
    if url.scheme() != "https" || url.host_str().is_none() {
        anyhow::bail!("OIDC {name} endpoint must use https")
    }
    Ok(())
}

fn validate_oidc_metadata(metadata: &CoreProviderMetadata) -> anyhow::Result<()> {
    validate_oidc_endpoint(metadata.authorization_endpoint().url(), "authorization")?;
    validate_oidc_endpoint(metadata.jwks_uri().url(), "JWKS")?;
    let token_endpoint = metadata
        .token_endpoint()
        .ok_or_else(|| anyhow::anyhow!("OIDC discovery did not provide a token endpoint"))?;
    validate_oidc_endpoint(token_endpoint.url(), "token")?;
    if metadata.id_token_signing_alg_values_supported().is_empty() {
        anyhow::bail!("OIDC discovery did not provide a usable ID Token signing algorithm")
    }
    Ok(())
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
    let issuer = normalized_oidc_issuer(&payload.issuer).map_err(auth_error)?;
    // Discovery is performed before persistence so invalid issuers never become active.
    let http = oidc_http_client().map_err(auth_error)?;
    CoreProviderMetadata::discover_async(
        IssuerUrl::new(issuer).map_err(|error| auth_error(error.into()))?,
        &http,
    )
    .await
    .map_err(|error| auth_error(anyhow::anyhow!(error.to_string())))
    .and_then(|metadata| validate_oidc_metadata(&metadata).map_err(auth_error))?;
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
            action: "oidc.provider_created",
            target_type: "oidc_provider",
            target_id: Some(provider.provider_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"issuer": provider.issuer}),
        })
        .await
        .map_err(auth_error)?;
    let value = redact_oidc_provider_secret(
        serde_json::to_value(provider).map_err(|error| auth_error(error.into()))?,
    );
    Ok((StatusCode::CREATED, Json(value)))
}

async fn list_oidc_providers(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let providers = state
        .identity
        .list_oidc_providers()
        .await
        .map_err(auth_error)?;
    let value = serde_json::to_value(providers).map_err(|error| auth_error(error.into()))?;
    let Value::Array(providers) = value else {
        return Err(auth_error(anyhow::anyhow!(
            "invalid OIDC provider response"
        )));
    };
    Ok(Json(Value::Array(
        providers
            .into_iter()
            .map(redact_oidc_provider_secret)
            .collect(),
    )))
}

async fn disable_oidc_provider(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(provider_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "recent node-admin Passkey session is required",
        ));
    }
    let provider = state
        .identity
        .disable_oidc_provider(identity.account_id, provider_id)
        .await
        .map_err(recovery_aware_auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(identity.request_identity.credential_id),
            action: "oidc.provider_disabled",
            target_type: "oidc_provider",
            target_id: Some(provider_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"issuer": provider.issuer}),
        })
        .await
        .map_err(auth_error)?;
    Ok(Json(redact_oidc_provider_secret(
        serde_json::to_value(provider).map_err(|error| auth_error(error.into()))?,
    )))
}

async fn list_oidc_links(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    Ok(Json(Value::Array(
        state
            .identity
            .list_oidc_links(identity.account_id)
            .await
            .map_err(auth_error)?,
    )))
}

async fn unlink_oidc(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(method_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    require_recent_passkey(&identity)?;
    state
        .identity
        .unlink_oidc(
            identity.account_id,
            identity.credential_generation,
            method_id,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    state
        .identity
        .append_node_audit(NodeAuditInput {
            subject_account_id: Some(identity.account_id),
            actor_account_id: Some(identity.account_id),
            credential_id: Some(identity.request_identity.credential_id),
            action: "oidc.identity_unlinked",
            target_type: "oidc_authentication_method",
            target_id: Some(method_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({}),
        })
        .await
        .map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct OidcStartQuery {
    invitation_token: Option<String>,
}

async fn oidc_start(
    State(state): State<AppState>,
    Path(provider_id): Path<Uuid>,
    Query(query): Query<OidcStartQuery>,
) -> ApiResult<Response> {
    let (redirect, state_hash) = start_oidc_authorization(
        &state,
        provider_id,
        query.invitation_token.as_deref(),
        None,
        None,
    )
    .await?;
    let mut response = redirect.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&oidc_state_cookie_header(&state_hash, 600))
            .map_err(|_| auth_error(anyhow::anyhow!("invalid OIDC state cookie")))?,
    );
    Ok(response)
}

async fn oidc_link_start(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(provider_id): Path<Uuid>,
) -> ApiResult<Response> {
    require_recent_passkey(&identity)?;
    let (redirect, state_hash) = start_oidc_authorization(
        &state,
        provider_id,
        None,
        Some(identity.account_id),
        identity.session_token.as_deref(),
    )
    .await?;
    let mut response = redirect.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&oidc_state_cookie_header(&state_hash, 600))
            .map_err(|_| auth_error(anyhow::anyhow!("invalid OIDC state cookie")))?,
    );
    Ok(response)
}

async fn start_bootstrap_passkey(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
) -> ApiResult<Json<Value>> {
    let session_token = identity.session_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "the invitation OIDC session is required",
        )
    })?;
    let result = state
        .identity
        .start_bootstrap_passkey(session_token, identity.account_id)
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok(Json(
        serde_json::to_value(result).map_err(|error| auth_error(error.into()))?,
    ))
}

async fn finish_bootstrap_passkey(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Json(payload): Json<AddPasskeyFinishRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let session_token = identity.session_token.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "the invitation OIDC session is required",
        )
    })?;
    let result = state
        .identity
        .finish_bootstrap_passkey(
            session_token,
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
            safe_metadata: json!({"bootstrap": true}),
        })
        .await
        .map_err(auth_error)?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn start_oidc_authorization(
    state: &AppState,
    provider_id: Uuid,
    invitation_token: Option<&str>,
    link_account_id: Option<Uuid>,
    initiating_session_token: Option<&str>,
) -> ApiResult<(Redirect, String)> {
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
    validate_oidc_metadata(&metadata).map_err(auth_error)?;
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
        .save_oidc_attempt_with_session(
            provider_id,
            csrf.secret(),
            nonce.secret(),
            pkce_verifier.secret(),
            invitation_token,
            link_account_id,
            initiating_session_token,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    Ok((
        Redirect::temporary(auth_url.as_str()),
        hex::encode(Sha256::digest(csrf.secret().as_bytes())),
    ))
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: Option<String>,
    state: String,
    error: Option<String>,
}

async fn oidc_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OidcCallbackQuery>,
) -> ApiResult<Response> {
    let expected_state_hash = hex::encode(Sha256::digest(query.state.as_bytes()));
    if oidc_state_cookie(&headers).as_deref() != Some(expected_state_hash.as_str()) {
        return Err(auth_error(anyhow::anyhow!(
            "OIDC browser state is not bound"
        )));
    }
    let attempt = state
        .identity
        .consume_oidc_attempt(&query.state)
        .await
        .map_err(recovery_aware_auth_error)?;
    if let OidcAttemptPurpose::Link {
        account_id,
        credential_generation,
    } = &attempt.purpose
    {
        let session_token = auth_session_cookie(&headers)
            .ok_or_else(|| auth_error(anyhow::anyhow!("OIDC link session is missing")))?;
        let actual_session_hash = hex::encode(Sha256::digest(session_token.as_bytes()));
        if attempt.initiating_session_hash.as_deref() != Some(actual_session_hash.as_str()) {
            return Err(auth_error(anyhow::anyhow!(
                "OIDC link session is not bound"
            )));
        }
        let session = state
            .identity
            .authenticate_session(&session_token)
            .await
            .map_err(recovery_aware_auth_error)?;
        if session.account.account_id != *account_id {
            return Err(auth_error(anyhow::anyhow!(
                "OIDC link account is not bound"
            )));
        }
        state
            .identity
            .revalidate_recent_passkey_session(
                &session_token,
                *account_id,
                session.credential_id,
                *credential_generation,
            )
            .await
            .map_err(recovery_aware_auth_error)?;
    }
    if query.error.is_some() {
        return Err(auth_error(anyhow::anyhow!("OIDC authorization was denied")));
    }
    let code = query
        .code
        .ok_or_else(|| auth_error(anyhow::anyhow!("OIDC provider omitted authorization code")))?;
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
    validate_oidc_metadata(&metadata).map_err(auth_error)?;
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
        .exchange_code(AuthorizationCode::new(code))
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
    let linked_existing_account = matches!(attempt.purpose, OidcAttemptPurpose::Link { .. });
    let completion = state
        .identity
        .complete_oidc_login(
            attempt.provider_id,
            &provider.issuer,
            subject,
            &attempt.purpose,
        )
        .await
        .map_err(recovery_aware_auth_error)?;
    if let Some(invitation) = completion.invitation {
        bind_invited_account(
            &state,
            &completion.account,
            &invitation,
            BindingMethod::Oidc,
        )
        .await?;
        state
            .identity
            .complete_invitation_acceptance(
                invitation.invitation_id,
                completion.account.account_id,
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
            subject_account_id: Some(completion.account.account_id),
            actor_account_id: Some(completion.account.account_id),
            credential_id: None,
            action: if linked_existing_account {
                "oidc.identity_linked"
            } else {
                "oidc.login"
            },
            target_type: "human_account",
            target_id: Some(completion.account.account_id.to_string()),
            outcome: "success",
            request_id: None,
            safe_metadata: json!({"issuer": provider.issuer}),
        })
        .await
        .map_err(auth_error)?;
    let location = if linked_existing_account {
        "/settings/security"
    } else if completion.passkey_bootstrap {
        "/settings/security?bootstrap=1"
    } else {
        "/spaces"
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(header::LOCATION, HeaderValue::from_static(location));
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear_oidc_state_cookie())
            .map_err(|_| auth_error(anyhow::anyhow!("invalid OIDC state cookie")))?,
    );
    if let Some(session_id) = completion.session_id {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&auth_cookie(&session_id, 60 * 60 * 24 * 30))
                .map_err(|_| auth_error(anyhow::anyhow!("invalid auth cookie")))?,
        );
    }
    Ok(response)
}

fn oidc_http_client() -> anyhow::Result<openidconnect::reqwest::Client> {
    Ok(openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
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
    reconcile_recovery_fences_api(state, &space_id).await?;
    let principal_id = invitation.accepted_principal_id().ok_or_else(|| {
        ApiError::new(StatusCode::CONFLICT, "invitation acceptance is incomplete")
    })?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (authorization, lease) = authorizer
        .acquire_state_lease(&space_id)
        .await
        .map_err(recovery_aware_auth_error)?;
    let fence = lease.write_fence();
    let result = match lease
        .run_while_held(|| {
            ugoite_iceberg::authorization::with_authorization_write_fence(
                fence.clone(),
                async {
                    if has_active_recovery_fence(&authorization) {
                        return Err(recovery_fence_unavailable());
                    }
            let active_space_member = authorization
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
            let existing_binding = state
                .identity
                .binding_for_account(space_uid, account.account_id)
                .await
                .map_err(auth_error)?;
            if existing_binding.is_some_and(|existing| existing != principal_id) {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    json!({
                        "code": "ACCOUNT_ALREADY_BOUND",
                        "message": "account is already bound to this Space",
                    }),
                ));
            }
            if principal_has_conflicting_space_state
                && (existing_binding.is_none()
                    || authorization.principals.contains_key(&principal_id))
            {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    json!({
                        "code": "SPACE_MEMBERSHIP_CONFLICT",
                        "message": "Space authorization state conflicts with the invitation principal",
                    }),
                ));
            }

            // Commit the Node half first. If the process exits before the
            // Space CAS below, retry sees the durable Node binding and can
            // finish the Space membership. A terminal Node rejection cannot
            // therefore strand an active Space member with no Node binding.
            if existing_binding.is_none() {
                ugoite_iceberg::authorization::ensure_authorization_write_fence()
                    .await
                    .map_err(recovery_aware_auth_error)?;
                state
                    .identity
                    .finalize_invitation_binding(
                        invitation.invitation_id,
                        account.account_id,
                        principal_id,
                        binding_method,
                    )
                    .await
                    .map_err(recovery_aware_auth_error)?;
            }
            if !active_space_member {
                let inviter = state
                    .identity
                    .principal_for_account(space_uid, invitation.created_by)
                    .await
                    .map_err(auth_error)?;
                authorizer
                    .add_human_member_with_lease(
                        &space_id,
                        inviter,
                        SpacePrincipal {
                            principal_id,
                            kind: PrincipalKind::Human,
                            state: PrincipalState::Active,
                            display_name: account.display_name.clone(),
                            created_at: chrono::Utc::now().to_rfc3339(),
                        },
                        parse_space_role(invitation.role.as_deref().unwrap_or("viewer"))?,
                        &lease,
                    )
                    .await
                    .map_err(recovery_aware_auth_error)?;
            }
            Ok(())
                },
            )
        })
        .await
    {
        Ok(result) => result,
        Err(()) => Err(recovery_fence_unavailable()),
    };
    let release = lease.release().await;
    match (result, release) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(ApiError::from_core(error)),
        (Err(error), Err(release_error)) => {
            eprintln!(
                "release Space authorization mutation lease after invitation failure: {release_error:#}"
            );
            Err(error)
        }
    }
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
    let issuer = issuer.trim_end_matches('/');
    Ok(Json(json!({
        "resource": format!("{issuer}/mcp"),
        "authorization_servers": [issuer],
        "scopes_supported": ["read", "create", "update", "delete"],
        "bearer_methods_supported": ["header"],
        "resource_documentation": OAUTH_RESOURCE_DOCUMENTATION_URL
    })))
}

fn api_base_url(issuer: &str) -> String {
    env::var("UGOITE_API_BASE_URL")
        .unwrap_or_else(|_| issuer.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub(crate) fn configured_cors_origin_allowed(origin: &str) -> bool {
    env::var("UGOITE_CORS_ALLOWED_ORIGINS").is_ok_and(|origins| {
        origins
            .split(',')
            .map(str::trim)
            .any(|configured| configured == origin)
    })
}

fn validate_mcp_resource(resource: &Option<String>, issuer: &str) -> ApiResult<()> {
    let expected = format!("{}/mcp", issuer.trim_end_matches('/'));
    if resource.as_deref().is_some_and(|value| value != expected) {
        return Err(invalid_oauth_target());
    }
    Ok(())
}

fn invalid_oauth_target() -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, json!({"error":"invalid_target"}))
}

fn validate_stored_oauth_resource(
    requested: Option<&String>,
    stored: Option<&String>,
) -> ApiResult<()> {
    if requested != stored {
        return Err(invalid_oauth_target());
    }
    Ok(())
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
        "grant_types_supported": ["authorization_code", "urn:ietf:params:oauth:grant-type:device_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["private_key_jwt"],
        "dpop_signing_alg_values_supported": ["ES256"],
        "scopes_supported": ["read", "create", "update", "delete", "share"],
        "authorization_response_iss_parameter_supported": true
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
    resource: Option<String>,
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
    Ok((public_key_jwk, actions))
}

async fn oauth_authorize(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Query(payload): Query<AuthorizePayload>,
) -> ApiResult<Html<String>> {
    require_recent_passkey(&identity)?;
    let (_, actions) = validate_authorize_payload(&payload)?;
    validate_mcp_resource(&payload.resource, state.identity.public_origin())?;
    if payload.resource.is_some() {
        validate_mcp_requested_actions(&actions)?;
    } else {
        validate_access_credential_actions(&actions)?;
    }
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
    if let Some(resource) = &payload.resource {
        fields.push_str(&hidden("resource", resource));
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
    validate_mcp_resource(&payload.resource, state.identity.public_origin())?;
    if payload.resource.is_some() {
        validate_mcp_requested_actions(&actions)?;
    } else {
        validate_access_credential_actions(&actions)?;
    }
    let space_uid = state
        .service
        .space_uid(&payload.space_id)
        .await
        .map_err(ApiError::from_core)?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let identity_service = state.identity.clone();
    let client_id = payload.client_id.clone();
    let redirect_uri = payload.redirect_uri.clone();
    let code_challenge = payload.code_challenge.clone();
    let resource = payload.resource.clone();
    let space_id_for_auth = payload.space_id.clone();
    let account_id = identity.account_id;
    let code = with_authorization_lease(&state, &payload.space_id, move |_lease| {
        Box::pin(async move {
            let principal_id = identity_service
                .principal_for_account(space_uid, account_id)
                .await
                .map_err(auth_error)?;
            let effective = authorizer
                .effective_actions(&space_id_for_auth, principal_id, None)
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
            ugoite_iceberg::authorization::ensure_authorization_write_fence()
                .await
                .map_err(ApiError::from_core)?;
            identity_service
                .issue_authorization_code(
                    &client_id,
                    &redirect_uri,
                    &code_challenge,
                    public_key_jwk,
                    account_id,
                    principal_id,
                    space_uid,
                    actions,
                    resource,
                )
                .await
                .map_err(auth_error)
        })
    })
    .await?;
    let mut redirect = url::Url::parse(&payload.redirect_uri)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid redirect_uri"))?;
    redirect
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &payload.state)
        .append_pair("iss", state.identity.public_origin());
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
    resource: Option<String>,
}

async fn oauth_device_authorization(
    State(state): State<AppState>,
    Json(mut payload): Json<DeviceAuthorizationPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_mcp_resource(&payload.resource, state.identity.public_origin())?;
    oauth::jwk_thumbprint(&payload.public_key_jwk).map_err(auth_error)?;
    if payload.requested_actions.is_empty() {
        let defaults = if payload.resource.is_some() {
            ["read"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        } else {
            ["read", "create", "update"]
                .into_iter()
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        };
        payload.requested_actions.extend(defaults);
    }
    validate_action_names(&payload.requested_actions)?;
    if payload.resource.is_some() {
        validate_mcp_requested_actions(&payload.requested_actions)?;
    } else {
        validate_access_credential_actions(&payload.requested_actions)?;
    }
    let response = state
        .identity
        .start_device_authorization(
            &payload.device_name,
            payload.public_key_jwk,
            payload.space_uid,
            payload.requested_actions,
            payload.resource,
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
        "resource": pending.resource,
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
    if payload.granted_actions.is_empty() {
        payload.granted_actions = pending.requested_actions.clone();
    }
    validate_action_names(&payload.granted_actions)?;
    if pending.resource.is_some() {
        validate_mcp_requested_actions(&payload.granted_actions)?;
    } else {
        validate_access_credential_actions(&payload.granted_actions)?;
    }
    if !payload
        .granted_actions
        .is_subset(&pending.requested_actions)
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "approval cannot expand requested actions",
        ));
    }
    let authorizer = Authorizer::new(state.service.operator().clone());
    let identity_service = state.identity.clone();
    let user_code = payload.user_code.clone();
    let granted_actions = payload.granted_actions.clone();
    let device_name = pending.device_name.clone();
    let account_id = identity.account_id;
    let space_id_for_auth = payload.space_id.clone();
    let (_principal_id, ()) = with_authorization_lease(&state, &payload.space_id, move |_lease| {
        Box::pin(async move {
            let principal_id = identity_service
                .principal_for_account(space_uid, account_id)
                .await
                .map_err(auth_error)?;
            let effective = authorizer
                .effective_actions(&space_id_for_auth, principal_id, None)
                .await
                .map_err(ApiError::from_core)?;
            for action in &granted_actions {
                let required = parse_action(action)?;
                if !effective.contains(&required) {
                    return Err(ApiError::new(
                        StatusCode::FORBIDDEN,
                        format!("principal cannot grant {action}"),
                    ));
                }
            }
            ugoite_iceberg::authorization::ensure_authorization_write_fence()
                .await
                .map_err(ApiError::from_core)?;
            identity_service
                .approve_device_authorization(
                    &user_code,
                    account_id,
                    principal_id,
                    space_uid,
                    granted_actions.clone(),
                )
                .await
                .map_err(auth_error)?;
            identity_service
                .append_node_audit(NodeAuditInput {
                    subject_account_id: Some(account_id),
                    actor_account_id: Some(account_id),
                    credential_id: None,
                    action: "oauth_grant.approved",
                    target_type: "cli_device_request",
                    target_id: None,
                    outcome: "success",
                    request_id: None,
                    safe_metadata: json!({
                        "space_uid": space_uid,
                        "device_name": device_name,
                        "granted_actions": granted_actions,
                    }),
                })
                .await
                .map_err(auth_error)?;
            Ok((principal_id, ()))
        })
    })
    .await?;
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
    resource: Option<String>,
}

async fn oauth_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> ApiResult<Json<Value>> {
    let payload: TokenPayload = decode_oauth_payload(&headers, request).await?;
    let (issuer, node_id) = state.identity.issuer_metadata().await.map_err(auth_error)?;
    let mcp_resource = format!("{}/mcp", issuer.trim_end_matches('/'));
    if let Some(resource) = &payload.resource {
        if resource != &mcp_resource {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_target"}),
            ));
        }
    }
    let audience = format!("{}/oauth/token", api_base_url(&issuer));
    let (credential, refresh, refresh_token, context) = if payload.grant_type
        == "authorization_code"
    {
        let code = payload
            .code
            .as_deref()
            .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "code is required"))?;
        let grant = state
            .identity
            .pending_authorization_code(code)
            .await
            .map_err(auth_error)?;
        validate_stored_oauth_resource(payload.resource.as_ref(), grant.resource.as_ref())?;
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
            .pending_device_by_device_code_for_resource(device_code, payload.resource.as_deref())
            .await
            .map_err(|error| match error.to_string().as_str() {
                "invalid_target" => invalid_oauth_target(),
                "slow_down" => {
                    ApiError::new(StatusCode::BAD_REQUEST, json!({"error": "slow_down"}))
                }
                message if message.contains("expired") => {
                    ApiError::new(StatusCode::BAD_REQUEST, json!({"error": "expired_token"}))
                }
                _ => auth_error(error),
            })?;
        validate_stored_oauth_resource(payload.resource.as_ref(), pending.resource.as_ref())?;
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
        let old_token = payload
            .refresh_token
            .as_deref()
            .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "refresh_token is required"))?;
        let old = state
            .identity
            .refresh_credential(old_token)
            .await
            .map_err(auth_error)?;
        validate_stored_oauth_resource(payload.resource.as_ref(), old.resource.as_ref())?;
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
    let issued_resource = context
        .get("resource")
        .and_then(Value::as_str)
        .map(str::to_string);
    if payload.resource != issued_resource {
        return Err(invalid_oauth_target());
    }
    let is_mcp = issued_resource.as_deref() == Some(mcp_resource.as_str());
    let now = chrono::Utc::now().timestamp();
    let thumbprint = oauth::jwk_thumbprint(&credential.public_key_jwk).map_err(auth_error)?;
    let claims = AccessTokenClaims {
        iss: context["issuer"].as_str().unwrap_or(&issuer).to_string(),
        node_id,
        sub: refresh.principal_id,
        principal_type: "human".to_string(),
        actor_principal_id: None,
        aud: if is_mcp { mcp_resource.clone() } else { issuer },
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
        "token_type": if is_mcp { "Bearer" } else { "DPoP" },
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
    reconcile_recovery_fences_api(&state, &space_id).await?;
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
    let agent_request = ugoite_iceberg::authorization::CreateAgentRequest {
        display_name: payload.display_name,
        description: payload.description,
        mode: payload.mode,
        owner_principal_ids: payload.owner_principal_ids,
        granted_actions: grants,
        expires_at: payload.expires_at.clone(),
    };
    let public_key_jwk = payload.public_key_jwk;
    let expires_at = payload.expires_at;
    let operator = state.service.operator().clone();
    let identity_service = state.identity.clone();
    let space_id_for_mutation = space_id.clone();
    let account_id = identity.account_id;
    let (agent, credential) = with_authorized_mutation_with_lease(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |lease, sponsor, _principals| {
            Box::pin(async move {
                let agent = Authorizer::new(operator)
                    .create_or_recover_agent_with_lease(
                        &space_id_for_mutation,
                        sponsor,
                        agent_request.clone(),
                        lease,
                    )
                    .await
                    .map_err(ApiError::from_core)?;
                ugoite_iceberg::authorization::ensure_authorization_write_fence()
                    .await
                    .map_err(ApiError::from_core)?;
                let credential = match identity_service
                    .agent_credential_for_agent(agent.agent_id)
                    .await
                    .map_err(auth_error)?
                {
                    Some(existing) if existing.public_key_jwk == public_key_jwk => existing,
                    Some(_) => {
                        return Err(ApiError::new(
                            StatusCode::CONFLICT,
                            "recovered agent already has a different active credential",
                        ));
                    }
                    None => identity_service
                        .register_agent_credential(agent.agent_id, public_key_jwk, expires_at)
                        .await
                        .map_err(auth_error)?,
                };
                identity_service
                    .append_node_audit(NodeAuditInput {
                        subject_account_id: Some(account_id),
                        actor_account_id: Some(account_id),
                        credential_id: Some(credential.credential_id),
                        action: "agent_credential.registered",
                        target_type: "agent",
                        target_id: Some(agent.agent_id.to_string()),
                        outcome: "success",
                        request_id: None,
                        safe_metadata: json!({"space_id": space_id_for_mutation}),
                    })
                    .await
                    .map_err(auth_error)?;
                Ok((agent, credential))
            })
        },
    )
    .await?;
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
    reconcile_recovery_fences_api(&state, &space_id).await?;
    let operator = state.service.operator().clone();
    let identity_service = state.identity.clone();
    let space_id_for_mutation = space_id.clone();
    let account_id = identity.account_id;
    with_authorized_mutation_with_lease(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |lease, actor, _principals| {
            Box::pin(async move {
                Authorizer::new(operator)
                    .revoke_agent_with_lease(&space_id_for_mutation, actor, agent_id, lease)
                    .await
                    .map_err(ApiError::from_core)?;
                ugoite_iceberg::authorization::ensure_authorization_write_fence()
                    .await
                    .map_err(ApiError::from_core)?;
                identity_service
                    .revoke_agent_credentials(agent_id)
                    .await
                    .map_err(auth_error)?;
                identity_service
                    .append_node_audit(NodeAuditInput {
                        subject_account_id: Some(account_id),
                        actor_account_id: Some(account_id),
                        credential_id: None,
                        action: "agent.revoked",
                        target_type: "agent",
                        target_id: Some(agent_id.to_string()),
                        outcome: "success",
                        request_id: None,
                        safe_metadata: json!({"space_id": space_id_for_mutation}),
                    })
                    .await
                    .map_err(auth_error)
            })
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AgentTokenPayload {
    credential_id: Uuid,
    client_assertion: String,
    space_id: String,
    #[serde(default)]
    requested_actions: BTreeSet<String>,
    resource: Option<String>,
}

async fn issue_autonomous_agent_token(
    State(state): State<AppState>,
    Json(payload): Json<AgentTokenPayload>,
) -> ApiResult<Json<Value>> {
    validate_mcp_resource(&payload.resource, state.identity.public_origin())?;
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
        payload.resource.as_deref(),
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
    validate_mcp_resource(&payload.resource, state.identity.public_origin())?;
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
        payload.resource.as_deref(),
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
    resource: Option<&str>,
    issuer: &str,
    node_id: Uuid,
) -> ApiResult<Json<Value>> {
    validate_action_names(&requested_actions)?;
    validate_access_credential_actions(&requested_actions)?;
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (authorization, lease) = authorizer
        .acquire_state_lease(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let result: ApiResult<Json<Value>> = async {
        let agent_actions = ugoite_iceberg::authorization::effective_actions_for_state(
            &authorization,
            agent_id,
            None,
        )
        .map_err(ApiError::from_core)?;
        let human_effective = if let Some(human) = on_behalf_of {
            Some(
                ugoite_iceberg::authorization::effective_actions_for_state(
                    &authorization,
                    human,
                    None,
                )
                .map_err(ApiError::from_core)?,
            )
        } else {
            None
        };
        let effective = human_effective
            .as_ref()
            .map(|human_actions| delegated_agent_actions(&agent_actions, human_actions))
            .unwrap_or_else(|| agent_actions.clone());
        if requested_actions.is_empty() {
            requested_actions = effective
                .iter()
                .map(|action| action_name(action).to_string())
                .collect();
        }
        if resource.is_some() {
            validate_mcp_requested_actions(&requested_actions)?;
        }
        for action in &requested_actions {
            let parsed = parse_action(action)?;
            let dangerous_scope = matches!(parsed, Action::Delete | Action::Share)
                && (agent_actions.contains(&parsed)
                    || authorization.policies.values().any(|policy| {
                        policy.grants.iter().any(|grant| {
                            grant.principal_id == agent_id && grant.actions.contains(&parsed)
                        })
                    }))
                && human_effective
                    .as_ref()
                    .is_none_or(|human_actions| human_actions.contains(&parsed));
            if !effective.contains(&parsed) && !dangerous_scope {
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
        let mut actor_chain = vec![agent_id];
        if let Some(human) = on_behalf_of {
            actor_chain.push(human);
        }
        let claims = build_agent_token_claims(
            issuer,
            node_id,
            agent_id,
            credential_id,
            public_key_jwk,
            requested_actions,
            actor_chain,
            space_uid,
            on_behalf_of,
            resource,
        )?;
        let claims_for_node = claims.clone();
        let access_token = match lease
            .run_while_held(|| {
                let fence = lease.write_fence();
                ugoite_iceberg::authorization::with_authorization_write_fence(fence, async {
                    ugoite_iceberg::authorization::ensure_authorization_write_fence()
                        .await
                        .map_err(ApiError::from_core)?;
                    let access_token = state
                        .identity
                        .issue_access_credential(claims_for_node)
                        .await
                        .map_err(auth_error)?;
                    state
                        .identity
                        .mark_agent_credential_used(credential_id)
                        .await
                        .map_err(auth_error)?;
                    Ok::<_, ApiError>(access_token)
                })
            })
            .await
        {
            Ok(access_token) => access_token?,
            Err(()) => {
                return Err(ApiError::from_core(anyhow::anyhow!(
                    "Space authorization mutation lease was lost"
                )))
            }
        };
        authorizer
            .mark_agent_used_with_lease(space_id, agent_id, &lease)
            .await
            .map_err(ApiError::from_core)?;
        Ok(Json(json!({
            "access_token": access_token,
            "token_type": "DPoP",
            "expires_in": 300,
            "space_uid": space_uid,
            "actor_chain": claims.actor_chain
        })))
    }
    .await;
    let release = lease.release().await;
    match (result, release) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(ApiError::from_core(error)),
        (Err(error), Err(_release_error)) => Err(error),
    }
}

fn agent_token_audience(issuer: &str, resource: Option<&str>) -> String {
    resource.unwrap_or(issuer).to_string()
}

#[allow(clippy::too_many_arguments)]
fn build_agent_token_claims(
    issuer: &str,
    node_id: Uuid,
    agent_id: Uuid,
    credential_id: Uuid,
    public_key_jwk: &Value,
    granted_actions: BTreeSet<String>,
    actor_chain: Vec<Uuid>,
    space_uid: Uuid,
    on_behalf_of: Option<Uuid>,
    resource: Option<&str>,
) -> ApiResult<AccessTokenClaims> {
    let now = chrono::Utc::now().timestamp();
    Ok(AccessTokenClaims {
        iss: issuer.to_string(),
        node_id,
        sub: on_behalf_of.unwrap_or(agent_id),
        principal_type: if on_behalf_of.is_some() {
            "human".to_string()
        } else {
            "agent".to_string()
        },
        actor_principal_id: Some(agent_id),
        aud: agent_token_audience(issuer, resource),
        space_uid,
        granted_actions,
        actor_chain,
        exp: now + 300,
        iat: now,
        jti: Uuid::now_v7(),
        credential_id,
        credential_generation: None,
        cnf: Confirmation {
            jkt: oauth::jwk_thumbprint(public_key_jwk).map_err(auth_error)?,
        },
    })
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
    // Delete/share are valid credential scopes. Dangerous routes additionally
    // require a single-use human approval bound to the exact mutation.
    let _ = actions;
    Ok(())
}

fn validate_mcp_requested_actions(actions: &BTreeSet<String>) -> ApiResult<()> {
    if actions.contains("share") {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "share is unavailable to MCP credentials",
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

fn require_actions_in_authorization_state(
    state: &AuthorizationState,
    principal_ids: &[Uuid],
    action: Action,
) -> anyhow::Result<()> {
    for principal_id in principal_ids {
        let actions =
            ugoite_iceberg::authorization::effective_actions_for_state(state, *principal_id, None)?;
        if !actions.contains(&action) {
            return Err(
                AppError::forbidden("principal is not authorized for the requested read").into(),
            );
        }
    }
    Ok(())
}

async fn require_resource_action(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
    kind: ResourceKind,
    resource_id: &str,
) -> ApiResult<Uuid> {
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

/// Runs a content mutation under the same authorization lock used by ACL
/// mutations. Local callers use the process lock; shared operators also hold
/// a heartbeat-backed object-store lease, so the permission check and the
/// authoritative write share one cross-process linearization window.
async fn with_authorized_mutation<T, F, Fut>(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
    resource: Option<ResourceRef>,
    operation: F,
) -> ApiResult<T>
where
    F: FnOnce(Uuid, Vec<Uuid>) -> Fut,
    Fut: Future<Output = ApiResult<T>>,
{
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    if let Some(actions) = &identity.token_actions {
        if !actions.contains(action_name(&action)) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token does not grant the required action",
            ));
        }
    }
    let principal_id = principal_for_space(state, space_id, identity).await?;
    let principals = authorization_principal_ids(identity, principal_id);
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (authorization_state, lease) = authorizer
        .acquire_state_lease(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let authorization_result = (|| -> ApiResult<()> {
        for subject in &principals {
            let actions = ugoite_iceberg::authorization::effective_actions_for_state(
                &authorization_state,
                *subject,
                resource.as_ref(),
            )
            .map_err(ApiError::from_core)?;
            if !actions.contains(&action) {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "principal is not authorized for the mutation",
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = authorization_result {
        let _ = lease.release().await;
        return Err(error);
    }
    let fence = lease.write_fence();
    let result = match lease
        .run_while_held(|| {
            ugoite_iceberg::authorization::with_authorization_write_fence(
                fence.clone(),
                operation(principal_id, principals),
            )
        })
        .await
    {
        Ok(result) => result,
        Err(()) => Err(ApiError::from_core(anyhow::anyhow!(
            "Space authorization mutation lease was lost"
        ))),
    };
    if let Err(error) = lease.release().await {
        return Err(ApiError::from_core(error));
    }
    result
}

/// Use this boundary when the service operation performs the authoritative
/// authorization check and acquires the Space lease itself. Holding the
/// server wrapper lease here would deadlock on the same non-reentrant mutex.
/// The wrapper still checks the request token and snapshot for early rejection;
/// the service method is the final protected check immediately before its
/// write.
async fn with_authorized_service_mutation<T, F, Fut>(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
    resource: Option<ResourceRef>,
    operation: F,
) -> ApiResult<T>
where
    F: FnOnce(Uuid, Vec<Uuid>) -> Fut,
    Fut: Future<Output = ApiResult<T>>,
{
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    if let Some(actions) = &identity.token_actions {
        if !actions.contains(action_name(&action)) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token does not grant the required action",
            ));
        }
    }
    let principal_id = principal_for_space(state, space_id, identity).await?;
    let principals = authorization_principal_ids(identity, principal_id);
    let authorization_state = Authorizer::new(state.service.operator().clone())
        .state(space_id)
        .await
        .map_err(ApiError::from_core)?;
    for subject in &principals {
        let actions = ugoite_iceberg::authorization::effective_actions_for_state(
            &authorization_state,
            *subject,
            resource.as_ref(),
        )
        .map_err(ApiError::from_core)?;
        if !actions.contains(&action) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "principal is not authorized for the mutation",
            ));
        }
    }
    operation(principal_id, principals).await
}

/// Lease-aware form of [`with_authorized_mutation`] for mutations that also
/// update the authorization document. The callback must reuse this exact
/// lease when it calls an Authorizer mutation; otherwise a shared backend
/// would try to acquire the same durable lock twice.
async fn with_authorized_mutation_with_lease<T, F>(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    action: Action,
    resource: Option<ResourceRef>,
    operation: F,
) -> ApiResult<T>
where
    F: for<'a> FnOnce(
        &'a ugoite_iceberg::authorization::AuthorizationLease,
        Uuid,
        Vec<Uuid>,
    ) -> Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'a>>,
{
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    if let Some(actions) = &identity.token_actions {
        if !actions.contains(action_name(&action)) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "access token does not grant the required action",
            ));
        }
    }
    let principal_id = principal_for_space(state, space_id, identity).await?;
    let principals = authorization_principal_ids(identity, principal_id);
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (authorization_state, lease) = authorizer
        .acquire_state_lease(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let authorization_result = (|| -> ApiResult<()> {
        for subject in &principals {
            let actions = ugoite_iceberg::authorization::effective_actions_for_state(
                &authorization_state,
                *subject,
                resource.as_ref(),
            )
            .map_err(ApiError::from_core)?;
            if !actions.contains(&action) {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "principal is not authorized for the mutation",
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = authorization_result {
        let _ = lease.release().await;
        return Err(error);
    }
    let fence = lease.write_fence();
    let result = match lease
        .run_while_held(|| {
            ugoite_iceberg::authorization::with_authorization_write_fence(
                fence.clone(),
                operation(&lease, principal_id, principals),
            )
        })
        .await
    {
        Ok(result) => result,
        Err(()) => Err(ApiError::from_core(anyhow::anyhow!(
            "Space authorization mutation lease was lost"
        ))),
    };
    if let Err(error) = lease.release().await {
        return Err(ApiError::from_core(error));
    }
    result
}

async fn with_authorization_lease<T, F>(
    state: &AppState,
    space_id: &str,
    operation: F,
) -> ApiResult<T>
where
    F: for<'a> FnOnce(
        &'a ugoite_iceberg::authorization::AuthorizationLease,
    ) -> Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'a>>,
{
    let authorizer = Authorizer::new(state.service.operator().clone());
    let lease = authorizer
        .acquire_state_lease(space_id)
        .await
        .map_err(ApiError::from_core)?
        .1;
    let fence = lease.write_fence();
    let result = match lease
        .run_while_held(|| {
            ugoite_iceberg::authorization::with_authorization_write_fence(
                fence.clone(),
                operation(&lease),
            )
        })
        .await
    {
        Ok(result) => result,
        Err(()) => Err(ApiError::from_core(anyhow::anyhow!(
            "Space authorization mutation lease was lost"
        ))),
    };
    if let Err(error) = lease.release().await {
        return Err(ApiError::from_core(error));
    }
    result
}

/// Form upsert has a state-dependent action: creating a new Form requires
/// `create`, while replacing an existing Form requires `update`. Resolve that
/// distinction only after acquiring the same lease as the authorization
/// check, otherwise a concurrent Form creation can turn a checked update into
/// an unchecked create (or vice versa).
async fn with_authorized_form_upsert<T, F, Fut>(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    form_name: &str,
    operation: F,
) -> ApiResult<T>
where
    F: FnOnce(&ugoite_iceberg::authorization::AuthorizationLease) -> Fut,
    Fut: Future<Output = ApiResult<T>>,
{
    let principal_id = principal_for_space(state, space_id, identity).await?;
    let principals = authorization_principal_ids(identity, principal_id);
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (authorization_state, lease) = authorizer
        .acquire_state_lease(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let forms = match state.service.list_forms(space_id).await {
        Ok(forms) => forms,
        Err(error) => {
            let _ = lease.release().await;
            return Err(ApiError::from_core(error));
        }
    };
    let existing = forms
        .into_iter()
        .any(|form| form.get("name").and_then(Value::as_str) == Some(form_name));
    let action = if existing {
        Action::Update
    } else {
        Action::Create
    };
    let token_allowed = identity
        .token_actions
        .as_ref()
        .is_none_or(|actions| actions.contains(action_name(&action)));
    let authorization_result = if !token_allowed {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "access token does not grant the required action",
        ))
    } else {
        let resource = existing.then(|| ResourceRef {
            kind: ResourceKind::Form,
            id: form_name.to_string(),
            parent: None,
        });
        (|| -> ApiResult<()> {
            for subject in &principals {
                let actions = ugoite_iceberg::authorization::effective_actions_for_state(
                    &authorization_state,
                    *subject,
                    resource.as_ref(),
                )
                .map_err(ApiError::from_core)?;
                if !actions.contains(&action) {
                    return Err(ApiError::new(
                        StatusCode::FORBIDDEN,
                        "principal is not authorized for the mutation",
                    ));
                }
            }
            Ok(())
        })()
    };
    if let Err(error) = authorization_result {
        let _ = lease.release().await;
        return Err(error);
    }
    let fence = lease.write_fence();
    let result = match lease
        .run_while_held(|| {
            ugoite_iceberg::authorization::with_authorization_write_fence(
                fence.clone(),
                operation(&lease),
            )
        })
        .await
    {
        Ok(result) => result,
        Err(()) => Err(ApiError::from_core(anyhow::anyhow!(
            "Space authorization mutation lease was lost"
        ))),
    };
    if let Err(error) = lease.release().await {
        return Err(ApiError::from_core(error));
    }
    result
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

fn active_credential_kind(identity: &RequestIdentityContext) -> ActiveCredentialKind {
    match identity.request_identity.authentication_method {
        RequestAuthenticationMethod::AgentAssertion => ActiveCredentialKind::Agent,
        RequestAuthenticationMethod::DeviceProof => ActiveCredentialKind::Device,
        RequestAuthenticationMethod::Passkey => ActiveCredentialKind::Passkey,
        RequestAuthenticationMethod::Oidc => ActiveCredentialKind::Oidc,
    }
}

async fn with_active_request_credential<T, F, Fut>(
    state: &AppState,
    identity: &RequestIdentityContext,
    operation: F,
) -> anyhow::Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let kind = active_credential_kind(identity);
    let account_id = if matches!(kind, ActiveCredentialKind::Agent) {
        None
    } else {
        Some(identity.account_id)
    };
    state
        .identity
        .with_active_credential(
            identity.request_identity.credential_id,
            account_id,
            kind,
            operation,
        )
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanApprovalIssuePayload {
    operation: String,
    mutation: Value,
    actor_credential_id: Uuid,
    expires_in_seconds: u64,
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        value => value.clone(),
    }
}

fn intent_hash(value: &Value) -> ApiResult<String> {
    let canonical = serde_json::to_vec(&canonical_json(value))
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

fn validate_approval_resource_id(value: &str, name: &str) -> ApiResult<()> {
    validate_id(value, name)
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.detail))
}

fn approval_intent(operation: &str, resource: &ResourceRef, intent: &Value) -> ApiResult<Value> {
    match operation {
        "entry.delete" => {
            let object = intent.as_object().ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete intent must be an object"}),
                )
            })?;
            let target_id = object.get("target_id").and_then(Value::as_str);
            let hard_delete = object.get("hard_delete").and_then(Value::as_bool);
            if object.len() != 2
                || target_id != Some(resource.id.as_str())
                || hard_delete.is_none()
                || hard_delete != Some(false)
            {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete intent does not match the operation and resource"}),
                ));
            }
            Ok(canonical_json(intent))
        }
        "sql.delete" | "asset.delete" => {
            let object = intent.as_object().ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete intent must be an object"}),
                )
            })?;
            let target_id = object.get("target_id").and_then(Value::as_str);
            if object.len() != 1 || target_id != Some(resource.id.as_str()) {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete intent does not match the operation and resource"}),
                ));
            }
            Ok(canonical_json(intent))
        }
        "access.put" => {
            let object = intent.as_object().ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation must be an object"}),
                )
            })?;
            let Some(kind) = object.get("kind").and_then(Value::as_str) else {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation.kind is required"}),
                ));
            };
            let Some(resource_id) = object.get("resource_id").and_then(Value::as_str) else {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation.resource_id is required"}),
                ));
            };
            if object.len() != 3
                || parse_resource_kind(kind)? != resource.kind
                || resource_id != resource.id
                || object.get("policy").and_then(Value::as_object).is_none()
            {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation does not match the resource"}),
                ));
            }
            let policy = serde_json::from_value::<AccessPolicy>(object["policy"].clone())
                .map_err(|error| {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":format!("invalid access policy: {error}")}),
                    )
                })?;
            let serialized_policy = serde_json::to_value(policy).map_err(|error| {
                ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string())
            })?;
            if canonical_json(&object["policy"]) != canonical_json(&serialized_policy) {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation.policy must be the complete canonical policy"}),
                ));
            }
            Ok(canonical_json(intent))
        }
        _ => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"operation is not eligible for human approval"}),
        )),
    }
}

fn approval_request_binding(
    operation: &str,
    mutation: &Value,
) -> ApiResult<(Action, ResourceRef, Value)> {
    let (action, kind, resource_id) = match operation {
        "entry.delete" => (Action::Delete, ResourceKind::Entry, "target_id"),
        "sql.delete" => (Action::Delete, ResourceKind::SavedSql, "target_id"),
        "asset.delete" => (Action::Delete, ResourceKind::Asset, "target_id"),
        "access.put" => {
            let object = mutation.as_object().ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation must be an object"}),
                )
            })?;
            let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation.kind is required"}),
                )
            })?;
            let resource_id = object
                .get("resource_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"access.put mutation.resource_id is required"}),
                    )
                })?;
            let resource_kind = parse_resource_kind(kind)?;
            let resource = ResourceRef {
                kind: resource_kind,
                id: resource_id.to_string(),
                parent: None,
            };
            validate_approval_resource_id(&resource.id, "resource_id")?;
            let intent = approval_intent(operation, &resource, mutation)?;
            return Ok((Action::Share, resource, intent));
        }
        _ => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"operation is not eligible for human approval"}),
            ));
        }
    };
    let object = mutation.as_object().ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete mutation must be an object"}),
        )
    })?;
    let target_id = object
        .get(resource_id)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"delete mutation.target_id is required"}),
            )
        })?;
    let resource = ResourceRef {
        kind,
        id: target_id.to_string(),
        parent: None,
    };
    validate_approval_resource_id(
        &resource.id,
        match &resource.kind {
            ResourceKind::SavedSql => "sql_id",
            ResourceKind::Asset => "asset_id",
            _ => "entry_id",
        },
    )?;
    let intent = approval_intent(operation, &resource, mutation)?;
    Ok((action, resource, intent))
}

fn approval_binding_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    match message.as_str() {
        "HUMAN_APPROVAL_EXPIRED" => ApiError::new(
            StatusCode::GONE,
            json!({"code":"HUMAN_APPROVAL_EXPIRED","message":message}),
        ),
        "HUMAN_APPROVAL_REPLAYED" => ApiError::new(
            StatusCode::CONFLICT,
            json!({"code":"HUMAN_APPROVAL_REPLAYED","message":message}),
        ),
        "HUMAN_APPROVAL_OUTCOME_UNKNOWN" => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"code":"HUMAN_APPROVAL_OUTCOME_UNKNOWN","message":"the approval state was durably changed but the mutation outcome is unknown; reconcile before retrying"}),
        ),
        "HUMAN_APPROVAL_INVALID" => ApiError::new(
            StatusCode::FORBIDDEN,
            json!({"code":"HUMAN_APPROVAL_INVALID","message":message}),
        ),
        _ => ApiError::from_core(error),
    }
}

fn human_approval_failure_phase(error: &anyhow::Error) -> &'static str {
    match error.to_string().as_str() {
        "HUMAN_APPROVAL_REPLAYED" => "replayed",
        "HUMAN_APPROVAL_EXPIRED" => "expired",
        "HUMAN_APPROVAL_OUTCOME_UNKNOWN" => "outcome_unknown",
        _ => "rejected",
    }
}

fn invalid_human_approval() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        json!({
            "code": "HUMAN_APPROVAL_INVALID",
            "message": "the human approval does not match the requested operation or mutation"
        }),
    )
}

fn invalid_human_approval_credential() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        json!({
            "code": "HUMAN_APPROVAL_INVALID",
            "message": "the human approval issuer credential is no longer active"
        }),
    )
}

fn approval_event_id(
    space_id: &str,
    approval_id: Option<Uuid>,
    phase: &str,
    outcome: &str,
    request_id: Uuid,
) -> Uuid {
    let approval = approval_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "none".to_string());
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("human-approval|{space_id}|{approval}|{phase}|{outcome}|{request_id}").as_bytes(),
    )
}

fn human_approval_audit_event(
    space_id: &str,
    approval: Option<&HumanApproval>,
    fallback_subject_principal_id: Option<Uuid>,
    phase: &str,
    outcome: &str,
    mutation_outcome: &str,
    request_id: Uuid,
) -> (Uuid, Value) {
    human_approval_audit_event_with_details(
        space_id,
        approval,
        fallback_subject_principal_id,
        HumanApprovalAuditDetails::default(),
        phase,
        outcome,
        mutation_outcome,
        request_id,
    )
}

#[derive(Clone, Default)]
struct HumanApprovalAuditDetails {
    actor_principal_id: Option<Uuid>,
    actor_credential_id: Option<Uuid>,
    operation: Option<String>,
    action: Option<String>,
    canonical_resource: Option<String>,
    intent_hash: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn human_approval_audit_event_with_details(
    space_id: &str,
    approval: Option<&HumanApproval>,
    fallback_subject_principal_id: Option<Uuid>,
    details: HumanApprovalAuditDetails,
    phase: &str,
    outcome: &str,
    mutation_outcome: &str,
    request_id: Uuid,
) -> (Uuid, Value) {
    let approval_id = approval.map(|value| value.approval_id);
    let subject_principal_id =
        fallback_subject_principal_id.or_else(|| approval.map(|value| value.issuer_principal_id));
    let actor_principal_id = approval
        .map(|value| value.actor_principal_id)
        .or(details.actor_principal_id);
    let actor_credential_id = approval
        .map(|value| value.actor_credential_id)
        .or(details.actor_credential_id);
    let operation = approval
        .map(|value| value.operation.clone())
        .or(details.operation);
    let action = approval
        .map(|value| action_name(&value.action))
        .or(details.action.as_deref());
    let canonical_resource = approval
        .map(|value| value.resource.key())
        .or(details.canonical_resource);
    let approval_intent_hash = approval
        .map(|value| value.intent_hash.clone())
        .or(details.intent_hash);
    let event_id = approval_event_id(space_id, approval_id, phase, outcome, request_id);
    let event = json!({
        "event_id": event_id,
        "action": format!("human_approval.{phase}"),
        "subject_principal_id": subject_principal_id,
        "actor_principal_id": actor_principal_id,
        "issuer_principal_id": approval.map(|value| value.issuer_principal_id),
        "issuer_account_id": approval.map(|value| value.issuer_account_id),
        "issuer_credential_id": approval.map(|value| value.issuer_credential_id),
        "credential_id": actor_credential_id,
        "target_type": "human_approval",
        "target_id": approval_id,
        "outcome": outcome,
        "request_id": request_id,
        "metadata": {
            "approval_id": approval_id,
            "operation": operation,
            "action": action,
            "canonical_resource": canonical_resource,
            "intent_hash": approval_intent_hash,
            "actor_credential_id": actor_credential_id,
            "issuer_credential_id": approval.map(|value| value.issuer_credential_id),
            "mutation_outcome": mutation_outcome,
        }
    });
    (event_id, event)
}

fn mutation_outcome(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<AppError>().map(AppError::kind) {
        Some(ErrorKind::InvalidInput)
        | Some(ErrorKind::Forbidden)
        | Some(ErrorKind::NotFound)
        | Some(ErrorKind::Conflict)
        | Some(ErrorKind::Expired)
        | Some(ErrorKind::Unimplemented) => "error",
        Some(ErrorKind::DependencyUnavailable | ErrorKind::Internal) | None => "unknown",
    }
}

fn dangerous_operation_audit_details(
    operation: &str,
    action: &Action,
    kind: &ResourceKind,
    resource_id: &str,
    intent: &Value,
    principal_id: Uuid,
    credential_id: Uuid,
) -> HumanApprovalAuditDetails {
    HumanApprovalAuditDetails {
        actor_principal_id: Some(principal_id),
        actor_credential_id: Some(credential_id),
        operation: Some(operation.to_string()),
        action: Some(action_name(action).to_string()),
        canonical_resource: Some(
            ResourceRef {
                kind: kind.clone(),
                id: resource_id.to_string(),
                parent: None,
            }
            .key(),
        ),
        intent_hash: intent_hash(intent).ok(),
    }
}

#[derive(Clone)]
struct PendingHumanApproval {
    approval: HumanApproval,
    token: String,
    operation: String,
    action: Action,
    resource: ResourceRef,
    intent_hash: String,
}

async fn append_human_approval_audit(
    state: &AppState,
    space_id: &str,
    approval: Option<&HumanApproval>,
    phase: &str,
    outcome: &str,
    mutation_outcome: &str,
    request_id: Uuid,
) -> anyhow::Result<()> {
    append_human_approval_audit_with_subject(
        state,
        space_id,
        approval,
        None,
        phase,
        outcome,
        mutation_outcome,
        request_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn append_human_approval_audit_with_subject(
    state: &AppState,
    space_id: &str,
    approval: Option<&HumanApproval>,
    fallback_subject_principal_id: Option<Uuid>,
    phase: &str,
    outcome: &str,
    mutation_outcome: &str,
    request_id: Uuid,
) -> anyhow::Result<()> {
    append_human_approval_audit_with_details(
        state,
        space_id,
        approval,
        fallback_subject_principal_id,
        HumanApprovalAuditDetails::default(),
        phase,
        outcome,
        mutation_outcome,
        request_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn append_human_approval_audit_with_details(
    state: &AppState,
    space_id: &str,
    approval: Option<&HumanApproval>,
    fallback_subject_principal_id: Option<Uuid>,
    details: HumanApprovalAuditDetails,
    phase: &str,
    outcome: &str,
    mutation_outcome: &str,
    request_id: Uuid,
) -> anyhow::Result<()> {
    let authorizer = Authorizer::new(state.service.operator().clone());
    let (event_id, event) = human_approval_audit_event_with_details(
        space_id,
        approval,
        fallback_subject_principal_id,
        details,
        phase,
        outcome,
        mutation_outcome,
        request_id,
    );
    authorizer
        .queue_human_approval_audit(space_id, event_id, event.clone())
        .await?;
    // A prior append may have failed after its durable queue write. Drain the
    // entire causal queue, including this event, in sequence order so a
    // later request cannot leapfrog an earlier approval lifecycle event.
    reconcile_human_approval_audit_outbox(state, space_id).await?;
    Ok(())
}

async fn issue_human_approval(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<HumanApprovalIssuePayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    require_recent_passkey(&identity)?;
    reconcile_human_approval_audit_outbox(&state, &space_id)
        .await
        .map_err(ApiError::from_core)?;
    validate_id(&space_id, "space_id")?;
    let (action, resource, intent) =
        approval_request_binding(&payload.operation, &payload.mutation)?;
    let issuer = principal_for_space(&state, &space_id, &identity).await?;
    let space_uid = state
        .service
        .space_uid(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let request_credential_id = identity.request_identity.credential_id;
    let issuer_account_id = identity.account_id;
    let issuer_credential_generation = identity.credential_generation;
    let request_id = identity.request_id;
    let actor_credential_id = payload.actor_credential_id;
    let operation = payload.operation;
    let ttl = chrono::Duration::seconds(payload.expires_in_seconds as i64);
    let approval_intent_hash = intent_hash(&intent)?;
    let approval_action = action.clone();
    let approval_resource = resource.clone();
    let approval_space_id = space_id.clone();
    let identity_service = state.identity.clone();
    let operator = state.service.operator().clone();
    let (approval, token) = with_authorized_mutation_with_lease(
        &state,
        &space_id,
        &identity,
        action,
        Some(resource),
        move |lease, _issuer_principal, _principals| {
            Box::pin(async move {
                identity_service
                    .with_active_human_approval_issuance(
                        request_credential_id,
                        Some(issuer_account_id),
                        ActiveCredentialKind::Passkey,
                        issuer_account_id,
                        request_credential_id,
                        issuer_credential_generation,
                        actor_credential_id,
                        space_uid,
                        move |actor_principal_id, issuer_node_account_lifecycle_epoch| {
                            let request = HumanApprovalIssue {
                                operation,
                                action: approval_action,
                                resource: approval_resource,
                                intent_hash: approval_intent_hash,
                                actor_principal_id,
                                actor_credential_id,
                                issuer_principal_id: issuer,
                                issuer_account_id,
                                issuer_credential_id: request_credential_id,
                                issuer_credential_generation,
                                issuer_node_account_lifecycle_epoch,
                                ttl,
                            };
                            let authorizer = Authorizer::new(operator.clone());
                            let audit_space_id = approval_space_id.clone();
                            let audit_event_space_id = audit_space_id.clone();
                            async move {
                                authorizer
                                    .issue_human_approval_with_audit_with_lease(
                                        &audit_space_id,
                                        request,
                                        move |approval| {
                                            vec![human_approval_audit_event(
                                                &audit_event_space_id,
                                                Some(approval),
                                                None,
                                                "issued",
                                                "success",
                                                "success",
                                                request_id,
                                            )]
                                        },
                                        lease,
                                    )
                                    .await
                            }
                        },
                    )
                    .await
                    .map_err(|error| {
                        if error.to_string() == "HUMAN_APPROVAL_ACTOR_INVALID" {
                            ApiError::new(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                json!({"code":"HUMAN_APPROVAL_INPUT_INVALID","message":"actor credential is not an active CLI device or agent credential bound to this Space"}),
                            )
                        } else {
                            ApiError::from_core(error)
                        }
                    })
            })
        },
    )
    .await?;
    let audit_status = append_human_approval_audit(
        &state,
        &space_id,
        Some(&approval),
        "issued",
        "success",
        "success",
        identity.request_id,
    )
    .await
    .is_ok();
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "approval_id": approval.approval_id,
            "approval_token": token,
            "operation": approval.operation,
            "action": action_name(&approval.action),
            "resource": {
                "kind": approval.resource.kind,
                "id": approval.resource.id,
            },
            "intent_hash": approval.intent_hash,
            "actor_principal_id": approval.actor_principal_id,
            "actor_credential_id": approval.actor_credential_id,
            "expires_at": approval.expires_at,
            "audit_status": if audit_status { "delivered" } else { "pending" },
        })),
    ))
}

#[allow(clippy::too_many_arguments)]
async fn require_dangerous_resource_action(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    operation: &str,
    action: Action,
    kind: ResourceKind,
    resource_id: &str,
    intent: &Value,
) -> ApiResult<(Uuid, Option<PendingHumanApproval>)> {
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    let audit_details = dangerous_operation_audit_details(
        operation,
        &action,
        &kind,
        resource_id,
        intent,
        identity
            .token_actor_principal_id
            .unwrap_or(identity.account_id),
        identity.request_identity.credential_id,
    );
    let principal_id = match require_resource_action(
        state,
        space_id,
        identity,
        action.clone(),
        kind.clone(),
        resource_id,
    )
    .await
    {
        Ok(principal_id) => principal_id,
        Err(error) => {
            // Authorization remains the first security decision, but a
            // dangerous-operation denial must still leave a durable lifecycle
            // event. Do not fabricate a target or approval id when the caller
            // never passed the resource ACL.
            let subject = principal_for_space(state, space_id, identity)
                .await
                .ok()
                .or(identity.token_actor_principal_id)
                .or(identity.token_principal_id);
            if let Some(subject) = subject {
                append_human_approval_audit_with_details(
                    state,
                    space_id,
                    None,
                    Some(subject),
                    audit_details.clone(),
                    "rejected",
                    "error",
                    "error",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
            return Err(error);
        }
    };
    reconcile_human_approval_audit_outbox(state, space_id)
        .await
        .map_err(ApiError::from_core)?;
    if identity.token_principal_id.is_some() && identity.human_approval_header_invalid {
        append_human_approval_audit_with_details(
            state,
            space_id,
            None,
            Some(principal_id),
            audit_details.clone(),
            "rejected",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(invalid_human_approval());
    }
    if identity.token_principal_id.is_some() && identity.human_approval_token.is_none() {
        append_human_approval_audit_with_details(
            state,
            space_id,
            None,
            Some(principal_id),
            audit_details.clone(),
            "required",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            json!({"code":"HUMAN_APPROVAL_REQUIRED","message":"a single-use human approval is required for this operation"}),
        ));
    }
    if identity.human_approval_header_invalid {
        append_human_approval_audit_with_details(
            state,
            space_id,
            None,
            Some(principal_id),
            audit_details.clone(),
            "rejected",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(invalid_human_approval());
    }
    if identity.human_approval_token.is_none() {
        if let Err(error) = require_recent_passkey(identity) {
            append_human_approval_audit_with_details(
                state,
                space_id,
                None,
                Some(principal_id),
                audit_details.clone(),
                "rejected",
                "error",
                "error",
                identity.request_id,
            )
            .await
            .map_err(ApiError::from_core)?;
            return Err(error);
        }
        return Ok((principal_id, None));
    }
    let token = identity
        .human_approval_token
        .as_deref()
        .expect("token was required for token identity");
    let resource = ResourceRef {
        kind: kind.clone(),
        id: resource_id.to_string(),
        parent: None,
    };
    let authorizer = Authorizer::new(state.service.operator().clone());
    let candidate = authorizer
        .human_approval_for_token(space_id, token)
        .await
        .map_err(ApiError::from_core)?;
    let normalized_intent = match approval_intent(operation, &resource, intent) {
        Ok(intent) => intent,
        Err(_) => {
            append_human_approval_audit_with_details(
                state,
                space_id,
                candidate.as_ref(),
                Some(principal_id),
                audit_details,
                "rejected",
                "error",
                "error",
                identity.request_id,
            )
            .await
            .map_err(ApiError::from_core)?;
            return Err(invalid_human_approval());
        }
    };
    let normalized_intent_hash =
        intent_hash(&normalized_intent).map_err(|_| invalid_human_approval())?;
    let Some(approval) = candidate else {
        append_human_approval_audit_with_details(
            state,
            space_id,
            None,
            Some(principal_id),
            audit_details,
            "rejected",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(invalid_human_approval());
    };
    if approval.operation != operation
        || approval.action != action
        || approval.resource != resource
        || approval.intent_hash != normalized_intent_hash
    {
        append_human_approval_audit_with_details(
            state,
            space_id,
            Some(&approval),
            Some(principal_id),
            audit_details,
            "rejected",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(invalid_human_approval());
    }
    if approval.consumed_at.is_some() {
        append_human_approval_audit_with_details(
            state,
            space_id,
            Some(&approval),
            Some(principal_id),
            audit_details,
            "replayed",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(approval_binding_error(
            AppError::conflict(ErrorCode::InvalidInput, "HUMAN_APPROVAL_REPLAYED").into(),
        ));
    }
    let expires_at = DateTime::parse_from_rfc3339(&approval.expires_at)
        .map_err(|_| invalid_human_approval())?
        .with_timezone(&Utc);
    if expires_at <= Utc::now() {
        append_human_approval_audit_with_details(
            state,
            space_id,
            Some(&approval),
            Some(principal_id),
            audit_details,
            "expired",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(approval_binding_error(
            AppError::expired(ErrorCode::InvalidInput, "HUMAN_APPROVAL_EXPIRED").into(),
        ));
    }
    let space_uid = state
        .service
        .space_uid(space_id)
        .await
        .map_err(ApiError::from_core)?;
    let issuer_principal = state
        .identity
        .principal_for_account(space_uid, approval.issuer_account_id)
        .await
        .map_err(ApiError::from_core)?;
    if issuer_principal != approval.issuer_principal_id {
        append_human_approval_audit_with_details(
            state,
            space_id,
            Some(&approval),
            Some(principal_id),
            audit_details,
            "rejected",
            "error",
            "error",
            identity.request_id,
        )
        .await
        .map_err(ApiError::from_core)?;
        return Err(invalid_human_approval());
    }
    Ok((
        principal_id,
        Some(PendingHumanApproval {
            approval,
            token: token.to_string(),
            operation: operation.to_string(),
            action,
            resource,
            intent_hash: normalized_intent_hash,
        }),
    ))
}

async fn execute_approved_mutation<T, F>(
    state: &AppState,
    space_id: &str,
    identity: &RequestIdentityContext,
    pending: PendingHumanApproval,
    mutation: F,
) -> anyhow::Result<(HumanApproval, anyhow::Result<T>)>
where
    F: for<'a> FnOnce(
        &'a ugoite_iceberg::authorization::AuthorizationLease,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>,
{
    let approval = pending.approval.clone();
    let token = pending.token;
    let operation = pending.operation;
    let action = pending.action;
    let resource = pending.resource;
    let intent_hash = pending.intent_hash;
    let space_id = space_id.to_string();
    let actor_principal_id = identity
        .token_actor_principal_id
        .unwrap_or(approval.actor_principal_id);
    let actor_credential_id = identity.request_identity.credential_id;
    let principal_id = identity
        .token_principal_id
        .unwrap_or(approval.actor_principal_id);
    let request_id = identity.request_id;
    let kind = active_credential_kind(identity);
    let account_id = if matches!(kind, ActiveCredentialKind::Agent) {
        None
    } else {
        Some(identity.account_id)
    };
    state
        .identity
        .with_active_approval_credentials(
            actor_credential_id,
            account_id,
            kind,
            approval.issuer_account_id,
            approval.issuer_credential_id,
            approval.issuer_credential_generation,
            approval.issuer_node_account_lifecycle_epoch,
            move || {
                let authorizer = Authorizer::new(state.service.operator().clone());
                let audit_space_id = space_id.clone();
                async move {
                    authorizer
                        .consume_human_approval_with_audit_and_with_lease(
                            &space_id,
                            &token,
                            &operation,
                            action,
                            &resource,
                            &intent_hash,
                            actor_principal_id,
                            actor_credential_id,
                            move |approval, phase, outcome, mutation_outcome| {
                                vec![human_approval_audit_event(
                                    &audit_space_id,
                                    approval,
                                    Some(principal_id),
                                    phase,
                                    outcome,
                                    mutation_outcome,
                                    request_id,
                                )]
                            },
                            mutation,
                        )
                        .await
                }
            },
        )
        .await
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
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let resource_kind = parse_resource_kind(&kind)?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let resource = ugoite_iceberg::authorization::ResourceRef {
        kind: resource_kind,
        id: resource_id,
        parent: None,
    };
    let policy = state
        .service
        .get_access_policy_authorized_for_principals(&space_id, &principals, resource)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(policy))
}

async fn put_access_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, kind, resource_id)): Path<(String, String, String)>,
    policy_payload: Result<Json<Value>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(policy_value) = policy_payload.map_err(access_policy_json_rejection)?;
    reconcile_recovery_fences_api(&state, &space_id).await?;
    let policy: AccessPolicy = serde_json::from_value(policy_value.clone()).map_err(|error| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"code":"INVALID_INPUT","message":format!("invalid access policy: {error}")}),
        )
    })?;
    let actor = principal_for_space(&state, &space_id, &identity).await?;
    let mutation_actor = identity.token_actor_principal_id.unwrap_or(actor);
    let resource = ugoite_iceberg::authorization::ResourceRef {
        kind: parse_resource_kind(&kind)?,
        id: resource_id.clone(),
        parent: None,
    };
    let approval_mutation = json!({
        "kind": kind,
        "resource_id": resource_id,
        "policy": policy_value,
    });
    let (_, approval) = require_dangerous_resource_action(
        &state,
        &space_id,
        &identity,
        "access.put",
        Action::Share,
        resource.kind.clone(),
        &resource.id,
        &approval_mutation,
    )
    .await?;
    let approval_for_audit = approval.as_ref().map(|pending| pending.approval.clone());
    let mutation = if let Some(pending) = approval {
        let audit_approval = pending.approval.clone();
        let policy_operator = state.service.operator().clone();
        let policy_space_id = space_id.clone();
        let policy_resource = resource.clone();
        let policy_value_for_mutation = policy.clone();
        let result =
            execute_approved_mutation(&state, &space_id, &identity, pending, move |lease| {
                Box::pin(async move {
                    Authorizer::new(policy_operator)
                        .set_policy_without_audit_with_lease(
                            &policy_space_id,
                            mutation_actor,
                            &policy_resource,
                            policy_value_for_mutation,
                            lease,
                        )
                        .await
                })
            })
            .await;
        let (_, mutation) = match result {
            Ok(value) => value,
            Err(error) => {
                let phase = human_approval_failure_phase(&error);
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(&audit_approval),
                    Some(actor),
                    phase,
                    "error",
                    "error",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
                return Err(if error.to_string().starts_with("human approval ") {
                    invalid_human_approval_credential()
                } else {
                    approval_binding_error(error)
                });
            }
        };
        mutation
    } else {
        let policy_operator = state.service.operator().clone();
        let policy_space_id = space_id.clone();
        let policy_resource = resource.clone();
        let policy_value_for_mutation = policy.clone();
        match with_authorized_mutation_with_lease(
            &state,
            &space_id,
            &identity,
            Action::Share,
            Some(policy_resource.clone()),
            move |lease, _actor, _principals| {
                Box::pin(async move {
                    Authorizer::new(policy_operator)
                        .set_policy_with_lease(
                            &policy_space_id,
                            mutation_actor,
                            &policy_resource,
                            policy_value_for_mutation,
                            lease,
                        )
                        .await
                        .map_err(ApiError::from_core)
                })
            },
        )
        .await
        {
            Ok(()) => Ok(()),
            Err(error) => return Err(error),
        }
    };
    match mutation {
        Ok(()) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(actor),
                    "mutation_succeeded",
                    "success",
                    "success",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
        }
        Err(error) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(actor),
                    "mutation_failed",
                    "error",
                    mutation_outcome(&error),
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
            return Err(ApiError::from_core(error));
        }
    }
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
        let Ok(principal_id) = require_space_action(&state, &id, &identity, Action::Read).await
        else {
            continue;
        };
        let principals = authorization_principal_ids(&identity, principal_id);
        let id_for_read = id.clone();
        let service = state.service.clone();
        let value = Authorizer::new(service.operator().clone())
            .with_state_lock(&id, move |authorization| async move {
                require_actions_in_authorization_state(&authorization, &principals, Action::Read)?;
                service.get_space(&id_for_read).await
            })
            .await
            .map(sanitize_space_response)
            .map_err(ApiError::from_core)?;
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
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    require_recent_passkey(&identity)?;
    validate_id(&payload.name, "space_id")?;
    if !identity.node_admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "node admin role is required to create a Space",
        ));
    }
    let (space_uid, created) = ensure_local_space_owner_binding(
        &state,
        &payload.name,
        identity.account_id,
        &identity.display_name,
    )
    .await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({
            "id": space_uid,
            "slug": payload.name,
            "space_uid": space_uid,
            "name": payload.name,
            "path": state.workspace(&space_uid.to_string())
        })),
    ))
}

/// Completes Space creation after an interrupted local bootstrap. The
/// scaffold and authorization owner are durable enough to identify the retry
/// target; the Node binding is then an idempotent final step. This is limited
/// to the authenticated Node-administrator route.
async fn ensure_local_space_owner_binding(
    state: &AppState,
    slug: &str,
    account_id: Uuid,
    display_name: &str,
) -> ApiResult<(Uuid, bool)> {
    Authorizer::new(state.service.operator().clone())
        .ensure_authoritative_mutation_contract()
        .map_err(ApiError::from_core)?;
    Authorizer::new(state.service.operator().clone())
        .verify_authoritative_storage(slug)
        .await
        .map_err(ApiError::from_core)?;
    if let Some(existing_id) = state
        .service
        .recover_space_id_by_slug(slug)
        .await
        .map_err(ApiError::from_core)?
    {
        let space_uid = state
            .service
            .space_uid(&existing_id)
            .await
            .map_err(ApiError::from_core)?;
        space::validate_complete_bootstrap(state.service.operator(), &existing_id)
            .await
            .map_err(ApiError::from_core)?;
        let authorizer = Authorizer::new(state.service.operator().clone());
        // This validates the current authorization layout and space UID. It
        // initializes ownership only when the authorization file is genuinely
        // absent; malformed, legacy, or mismatched state must fail closed.
        let principal_id = authorizer
            .ensure_owner(&existing_id, space_uid, display_name)
            .await
            .map_err(ApiError::from_core)?;
        if let Some(bound_principal) = state
            .identity
            .binding_for_account(space_uid, account_id)
            .await
            .map_err(auth_error)?
        {
            if bound_principal != principal_id {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "Node account is already bound to a different Space principal",
                ));
            }
            return Ok((space_uid, false));
        }
        state
            .identity
            .bind_local_owner(space_uid, principal_id, account_id)
            .await
            .map_err(auth_error)?;
        return Ok((space_uid, false));
    }

    let principal_id = Uuid::now_v7();
    let space_uid = state
        .service
        .create_space_for_principal(slug, principal_id, display_name)
        .await
        .map_err(ApiError::from_core)?;
    state
        .identity
        .bind_local_owner(space_uid, principal_id, account_id)
        .await
        .map_err(auth_error)?;
    Ok((space_uid, true))
}

async fn get_space(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let principal_id = require_space_action(&state, &space_id, &identity, Action::Read).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let space_id_for_read = space_id.clone();
    let value = Authorizer::new(state.service.operator().clone())
        .with_state_lock(&space_id, move |authorization| async move {
            require_actions_in_authorization_state(&authorization, &principals, Action::Read)?;
            state.service.get_space(&space_id_for_read).await
        })
        .await
        .map(sanitize_space_response)
        .map_err(ApiError::from_core)?;
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
    let principal_id = require_space_action(&state, &space_id, &identity, Action::Share).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let space_id_for_health = space_id.clone();
    let checkpoint_names = query.checkpoint;
    Authorizer::new(state.service.operator().clone())
        .with_state_lock(&space_id, move |authorization| async move {
            require_actions_in_authorization_state(&authorization, &principals, Action::Share)?;
            state
                .service
                .space_health(&space_id_for_health, &checkpoint_names)
                .await
        })
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
struct PinDiffQuery {
    from: String,
    to: String,
}

async fn pin_diff(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Query(query): Query<PinDiffQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    state
        .service
        .diff_pins_authorized_for_principals(&space_id, &query.from, &query.to, &principals)
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
    require_recent_passkey(&identity)?;
    let service = state.service.clone();
    let mutation_space_id = space_id.clone();
    let value = with_authorized_mutation(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |_principal_id, _principals| async move {
            service
                .patch_space(&mutation_space_id, &payload)
                .await
                .map(sanitize_space_response)
                .map_err(ApiError::from_core)
        },
    )
    .await?;
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
    let principal_id = require_space_action(&state, &space_id, &identity, Action::Share).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let space_id_for_read = space_id.clone();
    Authorizer::new(state.service.operator().clone())
        .with_state_lock(&space_id, move |authorization| async move {
            require_actions_in_authorization_state(&authorization, &principals, Action::Share)?;
            audit::list_audit_events(
                state.service.operator(),
                &space_id_for_read,
                AuditListOptions {
                    offset: query.offset,
                    limit: query.limit,
                    action: query.action,
                    actor_principal_id: query.actor_principal_id,
                    outcome: query.outcome,
                },
            )
            .await
        })
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
    let members = Authorizer::new(state.service.operator().clone())
        .with_state_lock(&space_id, |authorization| async move {
            Ok(authorization
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
                .collect::<Vec<_>>())
        })
        .await
        .map_err(ApiError::from_core)?;
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
    reconcile_recovery_fences_api(&state, &space_id).await?;
    require_recent_passkey(&identity)?;
    parse_space_role(&payload.role)?;
    let space_uid = state
        .service
        .space_uid(&space_id)
        .await
        .map_err(ApiError::from_core)?;
    let label = payload.label;
    let role = payload.role;
    let identity_service = state.identity.clone();
    let account_id = identity.account_id;
    let (invitation, token) = with_authorized_mutation(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |_principal_id, _principals| async move {
            identity_service
                .issue_invitation(account_id, &label, Some(space_uid), Some(role))
                .await
                .map_err(recovery_aware_auth_error)
        },
    )
    .await?;
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
    reconcile_recovery_fences_api(&state, &space_id).await?;
    require_recent_passkey(&identity)?;
    let role = parse_space_role(&payload.role)?;
    let operator = state.service.operator().clone();
    let space_id_for_mutation = space_id.clone();
    let role_for_mutation = role.clone();
    with_authorized_mutation_with_lease(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |lease, actor, _principals| {
            Box::pin(async move {
                Authorizer::new(operator)
                    .change_role_with_lease(
                        &space_id_for_mutation,
                        actor,
                        principal_id,
                        role_for_mutation,
                        lease,
                    )
                    .await
                    .map_err(recovery_aware_auth_error)
            })
        },
    )
    .await?;
    Ok(Json(json!({"principal_id": principal_id, "role": role})))
}

async fn revoke_member(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, principal_id)): Path<(String, Uuid)>,
) -> ApiResult<Json<Value>> {
    reconcile_recovery_fences_api(&state, &space_id).await?;
    require_recent_passkey(&identity)?;
    let operator = state.service.operator().clone();
    let space_id_for_mutation = space_id.clone();
    with_authorized_mutation_with_lease(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |lease, actor, _principals| {
            Box::pin(async move {
                Authorizer::new(operator)
                    .revoke_principal_with_lease(&space_id_for_mutation, actor, principal_id, lease)
                    .await
                    .map_err(recovery_aware_auth_error)
            })
        },
    )
    .await?;
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
    if payload.sql.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "sql is required",
        ));
    }
    let sql = payload.sql;
    let parameters = payload.parameters;
    let parameter_types = payload.parameter_types;
    let service = state.service.clone();
    let mutation_space_id = space_id.clone();
    let value = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Read,
        None,
        move |_principal_id, principals| async move {
            service
                .create_sql_session_authorized_for_principals_with_parameters(
                    &mutation_space_id,
                    &principals,
                    &sql,
                    parameters,
                    parameter_types,
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(value)))
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
    let entry_id = payload.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_id(&entry_id, "entry_id")?;
    let entry_id_for_write = entry_id.clone();
    let markdown = payload.markdown.clone();
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let created = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Create,
        None,
        |principal_id, principals| async move {
            service
                .create_entry_authorized_for_principals(
                    &space_id_for_write,
                    &entry_id_for_write,
                    &markdown,
                    &principal_id.to_string(),
                    &principals,
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": entry_id, "revision_id": created["revision_id"]})),
    ))
}

async fn list_pins(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        state
            .service
            .list_pins(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn list_changes(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        state
            .service
            .list_changes(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

#[derive(Default, Deserialize)]
struct ChangeRevertRequest {
    run_id: Option<String>,
    message: Option<String>,
}

async fn revert_change(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, change_id)): Path<(String, String)>,
    Json(payload): Json<ChangeRevertRequest>,
) -> ApiResult<Json<Value>> {
    if change_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "change_id must not be blank",
        ));
    }
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let change_id_for_write = change_id.clone();
    let run_id = payload.run_id.clone();
    let message = payload.message.clone();
    let result = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        None,
        move |principal_id, _principals| async move {
            service
                .revert_change(
                    &space_id_for_write,
                    &change_id_for_write,
                    &principal_id.to_string(),
                    run_id.as_deref(),
                    message.as_deref(),
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok(Json(result))
}

async fn undo_run(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, run_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    if run_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "run_id must not be blank",
        ));
    }
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let run_id_for_write = run_id.clone();
    let result = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        None,
        move |principal_id, _principals| async move {
            service
                .undo_run(
                    &space_id_for_write,
                    &run_id_for_write,
                    &principal_id.to_string(),
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct ApplyRequest {
    operations: Vec<ApplyOperation>,
    run_id: Option<String>,
    message: Option<String>,
}

async fn apply_operations(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    Json(payload): Json<ApplyRequest>,
) -> ApiResult<Json<Value>> {
    let remove_entry_id = match payload.operations.as_slice() {
        [ApplyOperation::Remove { id }] => Some(id.clone()),
        operations
            if operations
                .iter()
                .any(|operation| matches!(operation, ApplyOperation::Remove { .. })) =>
        {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({
                    "code": "INVALID_INPUT",
                    "message": "apply remove must be submitted as one operation so its human approval is bound to the exact Entry"
                }),
            ));
        }
        _ => None,
    };
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let run_id = payload.run_id.clone();
    let message = payload.message.clone();
    let operations = payload.operations;
    if let Some(entry_id) = remove_entry_id {
        validate_id(&entry_id, "entry_id")?;
        let approval_mutation = json!({
            "target_id": entry_id,
            "hard_delete": false,
        });
        let (principal_id, approval) = require_dangerous_resource_action(
            &state,
            &space_id,
            &identity,
            "entry.delete",
            Action::Delete,
            ResourceKind::Entry,
            &entry_id,
            &approval_mutation,
        )
        .await?;
        let mutation_actor = identity
            .token_actor_principal_id
            .unwrap_or(principal_id)
            .to_string();
        let principal_ids = authorization_principal_ids(&identity, principal_id);
        if let Some(pending) = approval {
            let audit_approval = pending.approval.clone();
            let approved_service = service.clone();
            let approved_space_id = space_id_for_write.clone();
            let approved_run_id = run_id.clone();
            let approved_message = message.clone();
            let approved_actor = mutation_actor.clone();
            let approved_principal_ids = principal_ids.clone();
            let result =
                execute_approved_mutation(&state, &space_id, &identity, pending, move |_| {
                    Box::pin(async move {
                        approved_service
                            .apply_operations(
                                &approved_space_id,
                                operations,
                                &approved_actor,
                                &approved_principal_ids,
                                approved_run_id.as_deref(),
                                approved_message.as_deref(),
                            )
                            .await
                    })
                })
                .await;
            let (_, mutation) = match result {
                Ok(value) => value,
                Err(error) => {
                    let phase = human_approval_failure_phase(&error);
                    append_human_approval_audit_with_subject(
                        &state,
                        &space_id,
                        Some(&audit_approval),
                        Some(principal_id),
                        phase,
                        "error",
                        "error",
                        identity.request_id,
                    )
                    .await
                    .map_err(ApiError::from_core)?;
                    return Err(if error.to_string().starts_with("human approval ") {
                        invalid_human_approval_credential()
                    } else {
                        approval_binding_error(error)
                    });
                }
            };
            return match mutation {
                Ok(value) => {
                    append_human_approval_audit_with_subject(
                        &state,
                        &space_id,
                        Some(&audit_approval),
                        Some(principal_id),
                        "mutation_succeeded",
                        "success",
                        "success",
                        identity.request_id,
                    )
                    .await
                    .map_err(ApiError::from_core)?;
                    Ok(Json(value))
                }
                Err(error) => {
                    append_human_approval_audit_with_subject(
                        &state,
                        &space_id,
                        Some(&audit_approval),
                        Some(principal_id),
                        "mutation_failed",
                        "error",
                        mutation_outcome(&error),
                        identity.request_id,
                    )
                    .await
                    .map_err(ApiError::from_core)?;
                    Err(ApiError::from_core(error))
                }
            };
        }
        let result = with_authorized_service_mutation(
            &state,
            &space_id,
            &identity,
            Action::Delete,
            Some(ResourceRef {
                kind: ResourceKind::Entry,
                id: entry_id,
                parent: None,
            }),
            move |principal_id, principal_ids| async move {
                service
                    .apply_operations(
                        &space_id_for_write,
                        operations,
                        &principal_id.to_string(),
                        &principal_ids,
                        run_id.as_deref(),
                        message.as_deref(),
                    )
                    .await
                    .map_err(ApiError::from_core)
            },
        )
        .await?;
        return Ok(Json(result));
    }
    let result = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        None,
        move |principal_id, principals| async move {
            service
                .apply_operations(
                    &space_id_for_write,
                    operations,
                    &principal_id.to_string(),
                    &principals,
                    run_id.as_deref(),
                    message.as_deref(),
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct PinCreate {
    name: String,
}

async fn create_pin(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<PinCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_recent_passkey(&identity)?;
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let name = payload.name.clone();
    let command_id = publication_command_id(&headers, "pin-create", identity.request_id)?;
    let pin = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |principal_id, _principals| async move {
            service
                .create_pin(
                    &space_id_for_write,
                    &name,
                    &principal_id.to_string(),
                    &command_id,
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok((StatusCode::OK, Json(pin)))
}

async fn delete_pin(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, pin_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    require_recent_passkey(&identity)?;
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let pin_name_for_write = pin_name.clone();
    let command_id = publication_command_id(&headers, "pin-delete", identity.request_id)?;
    with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Share,
        None,
        move |_principal_id, _principals| async move {
            service
                .delete_pin(&space_id_for_write, &pin_name_for_write, &command_id)
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok(Json(json!({"name": pin_name, "status": "deleted"})))
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
    let mut value = if let Some(pin) = query.pin.as_deref() {
        state
            .service
            .entry_at_pin_authorized_for_principals(&space_id, &entry_id, pin, &principals)
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
    pin: Option<String>,
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
    validate_id(&entry_id, "entry_id")?;
    let entry_id_for_write = entry_id.clone();
    let markdown = payload.markdown.clone();
    let parent_revision_id = payload.parent_revision_id.clone();
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let value = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        Some(ResourceRef {
            kind: ResourceKind::Entry,
            id: entry_id.clone(),
            parent: None,
        }),
        |principal_id, principals| async move {
            service
                .update_entry_authorized_for_principals(
                    &space_id_for_write,
                    &entry_id_for_write,
                    &markdown,
                    parent_revision_id.as_deref(),
                    &principal_id.to_string(),
                    &principals,
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
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
    validate_id(&entry_id, "entry_id")?;
    let (principal_id, approval) = require_dangerous_resource_action(
        &state,
        &space_id,
        &identity,
        "entry.delete",
        Action::Delete,
        ResourceKind::Entry,
        &entry_id,
        &json!({"target_id": entry_id, "hard_delete": query.hard_delete.unwrap_or(false)}),
    )
    .await?;
    let approval_for_audit = approval.as_ref().map(|pending| pending.approval.clone());
    let mutation_actor = identity
        .token_actor_principal_id
        .unwrap_or(principal_id)
        .to_string();
    let mutation = if let Some(pending) = approval {
        let audit_approval = pending.approval.clone();
        let mutation_service = state.service.clone();
        let mutation_space_id = space_id.clone();
        let mutation_entry_id = entry_id.clone();
        let mutation_hard_delete = query.hard_delete.unwrap_or(false);
        let mutation_actor_for_approved = mutation_actor.clone();
        let result = execute_approved_mutation(&state, &space_id, &identity, pending, move |_| {
            Box::pin(async move {
                mutation_service
                    .delete_entry(
                        &mutation_space_id,
                        &mutation_entry_id,
                        mutation_hard_delete,
                        &mutation_actor_for_approved,
                    )
                    .await
            })
        })
        .await;
        let (_, mutation) = match result {
            Ok(value) => value,
            Err(error) => {
                let phase = human_approval_failure_phase(&error);
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(&audit_approval),
                    Some(principal_id),
                    phase,
                    "error",
                    "error",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
                return Err(if error.to_string().starts_with("human approval ") {
                    invalid_human_approval_credential()
                } else {
                    approval_binding_error(error)
                });
            }
        };
        mutation
    } else {
        with_authorized_mutation(
            &state,
            &space_id,
            &identity,
            Action::Delete,
            Some(ResourceRef {
                kind: ResourceKind::Entry,
                id: entry_id.clone(),
                parent: None,
            }),
            |_principal_id, _principals| async {
                with_active_request_credential(&state, &identity, || async {
                    state
                        .service
                        .delete_entry(
                            &space_id,
                            &entry_id,
                            query.hard_delete.unwrap_or(false),
                            &mutation_actor,
                        )
                        .await
                })
                .await
                .map_err(ApiError::from_core)
            },
        )
        .await?;
        return Ok(Json(json!({"id": entry_id, "status": "deleted"})));
    };
    match mutation {
        Ok(()) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_succeeded",
                    "success",
                    "success",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
        }
        Err(error) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_failed",
                    "error",
                    mutation_outcome(&error),
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
            return Err(ApiError::from_core(error));
        }
    }
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
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&entry_id, "entry_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let history = if let Some(pin) = query.pin.as_deref() {
        state
            .service
            .entry_history_at_pin(&space_id, &entry_id, pin, &principals)
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .entry_history_authorized_for_principals(&space_id, &entry_id, &principals)
            .await
            .map_err(ApiError::from_core)?
    };
    Ok(Json(history))
}

async fn entry_revision(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id, revision_id)): Path<(String, String, String)>,
    Query(query): Query<EntryReadQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&entry_id, "entry_id")?;
    validate_id(&revision_id, "revision_id")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let mut revision = if let Some(pin) = query.pin.as_deref() {
        state
            .service
            .entry_revision_at_pin(&space_id, &entry_id, &revision_id, pin, &principals)
            .await
            .map_err(ApiError::from_core)?
    } else {
        state
            .service
            .entry_revision_authorized_for_principals(
                &space_id,
                &entry_id,
                &revision_id,
                &principals,
            )
            .await
            .map_err(ApiError::from_core)?
    };
    let policy_history = revision
        .get("access_policy_history")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let policy_history: Vec<ugoite_iceberg::authorization::PolicyRevision> =
        serde_json::from_value(policy_history)
            .map_err(|error| ApiError::from_core(error.into()))?;
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
    pin: Option<String>,
}

async fn restore_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<RestoreEntry>,
) -> ApiResult<Json<Value>> {
    validate_id(&entry_id, "entry_id")?;
    validate_id(&payload.revision_id, "revision_id")?;
    let revision_id = payload.revision_id.clone();
    let pin = payload.pin.clone();
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let entry_id_for_write = entry_id.clone();
    let value = with_authorized_service_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        Some(ResourceRef {
            kind: ResourceKind::Entry,
            id: entry_id.clone(),
            parent: None,
        }),
        |principal_id, principals| async move {
            if let Some(pin) = pin.as_deref() {
                service
                    .restore_entry_from_pin_authorized_for_principals(
                        &space_id_for_write,
                        &entry_id_for_write,
                        &revision_id,
                        pin,
                        &principal_id.to_string(),
                        &principals,
                    )
                    .await
                    .map_err(ApiError::from_core)
            } else {
                service
                    .restore_entry_authorized_for_principals(
                        &space_id_for_write,
                        &entry_id_for_write,
                        &revision_id,
                        &principal_id.to_string(),
                        &principals,
                    )
                    .await
                    .map_err(ApiError::from_core)
            }
        },
    )
    .await?;
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
        .list_forms_authorized_for_principals(&space_id, &principals)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(Value::Array(forms)))
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
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&form_name, "form_name")?;
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    Ok(Json(
        state
            .service
            .get_form_authorized_for_principals(&space_id, &form_name, &principals)
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
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let payload_for_write = payload.clone();
    with_authorized_form_upsert(&state, &space_id, &identity, form_name, |_| async move {
        service
            .upsert_form(&space_id_for_write, &payload_for_write)
            .await
            .map_err(ApiError::from_core)
    })
    .await?;
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
    if query.q.len() > ugoite_iceberg::derived_relation::MAX_ASSET_TEXT_QUERY_BYTES {
        return Err(ApiError::from_core(
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                "search query exceeds the configured byte limit",
            )
            .into(),
        ));
    }
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
    let id = Uuid::new_v4().to_string();
    let id_for_write = id.clone();
    let payload_for_write = payload.clone();
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let value = with_authorized_mutation(
        &state,
        &space_id,
        &identity,
        Action::Create,
        None,
        |principal_id, _principals| async move {
            service
                .create_saved_sql(
                    &space_id_for_write,
                    &id_for_write,
                    &payload_for_write,
                    &principal_id.to_string(),
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
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
    validate_id(&sql_id, "sql_id")?;
    let parent_revision_id = payload.parent_revision_id.clone();
    let payload = payload.into_sql_payload();
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let sql_id_for_write = sql_id.clone();
    let value = with_authorized_mutation(
        &state,
        &space_id,
        &identity,
        Action::Update,
        Some(ResourceRef {
            kind: ResourceKind::SavedSql,
            id: sql_id.clone(),
            parent: None,
        }),
        |principal_id, _principals| async move {
            service
                .update_saved_sql(
                    &space_id_for_write,
                    &sql_id_for_write,
                    &payload,
                    &parent_revision_id,
                    &principal_id.to_string(),
                )
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
    Ok(Json(value))
}

async fn delete_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    validate_id(&sql_id, "sql_id")?;
    let (principal_id, approval) = require_dangerous_resource_action(
        &state,
        &space_id,
        &identity,
        "sql.delete",
        Action::Delete,
        ResourceKind::SavedSql,
        &sql_id,
        &json!({"target_id": sql_id}),
    )
    .await?;
    let approval_for_audit = approval.as_ref().map(|pending| pending.approval.clone());
    let mutation_actor = identity
        .token_actor_principal_id
        .unwrap_or(principal_id)
        .to_string();
    let mutation = if let Some(pending) = approval {
        let audit_approval = pending.approval.clone();
        let mutation_service = state.service.clone();
        let mutation_space_id = space_id.clone();
        let mutation_sql_id = sql_id.clone();
        let mutation_actor_for_approved = mutation_actor.clone();
        let result = execute_approved_mutation(&state, &space_id, &identity, pending, move |_| {
            Box::pin(async move {
                mutation_service
                    .delete_saved_sql(
                        &mutation_space_id,
                        &mutation_sql_id,
                        &mutation_actor_for_approved,
                    )
                    .await
            })
        })
        .await;
        let (_, mutation) = match result {
            Ok(value) => value,
            Err(error) => {
                let phase = human_approval_failure_phase(&error);
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(&audit_approval),
                    Some(principal_id),
                    phase,
                    "error",
                    "error",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
                return Err(if error.to_string().starts_with("human approval ") {
                    invalid_human_approval_credential()
                } else {
                    approval_binding_error(error)
                });
            }
        };
        mutation
    } else {
        with_authorized_mutation(
            &state,
            &space_id,
            &identity,
            Action::Delete,
            Some(ResourceRef {
                kind: ResourceKind::SavedSql,
                id: sql_id.clone(),
                parent: None,
            }),
            |_principal_id, _principals| async {
                with_active_request_credential(&state, &identity, || async {
                    state
                        .service
                        .delete_saved_sql(&space_id, &sql_id, &mutation_actor)
                        .await
                })
                .await
                .map_err(ApiError::from_core)
            },
        )
        .await?;
        return Ok(StatusCode::NO_CONTENT);
    };
    match mutation {
        Ok(()) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_succeeded",
                    "success",
                    "success",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
        }
        Err(error) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_failed",
                    "error",
                    mutation_outcome(&error),
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
            return Err(ApiError::from_core(error));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentityContext>,
    Path(space_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let field = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::new(error.status(), error.body_text()))?
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "file is required"))?;
    if field.name() != Some("file") {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "multipart field `file` is required",
        ));
    }
    let name = field.file_name().unwrap_or("asset").to_string();
    let media_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    if name.len() > ugoite_domain::entry::MAX_ASSET_REFERENCE_NAME_BYTES {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asset filename exceeds the maximum length",
        ));
    }
    if media_type.len() > ugoite_domain::entry::MAX_ASSET_REFERENCE_MEDIA_TYPE_BYTES {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asset media type exceeds the maximum length",
        ));
    }
    let bytes = field
        .bytes()
        .await
        .map_err(|error| ApiError::new(error.status(), error.body_text()))?;
    if bytes.len() > ugoite_iceberg::asset::MAX_ASSET_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "asset exceeds the maximum size",
        ));
    }
    if multipart
        .next_field()
        .await
        .map_err(|error| ApiError::new(error.status(), error.body_text()))?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "only the `file` multipart field is allowed",
        ));
    }
    let service = state.service.clone();
    let space_id_for_write = space_id.clone();
    let value = with_authorized_mutation(
        &state,
        &space_id,
        &identity,
        Action::Create,
        None,
        |_principal_id, _principals| async move {
            service
                .save_asset_with_media_type(&space_id_for_write, &name, &bytes, &media_type)
                .await
                .map_err(ApiError::from_core)
        },
    )
    .await?;
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
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
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
    let principal_id = principal_for_space(&state, &space_id, &identity).await?;
    let principals = authorization_principal_ids(&identity, principal_id);
    let content = state
        .service
        .read_asset_authorized_for_principals(
            &space_id,
            &form_name,
            &entry_id,
            &asset_id,
            &principals,
        )
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
    validate_id(&asset_id, "asset_id")?;
    let (principal_id, approval) = require_dangerous_resource_action(
        &state,
        &space_id,
        &identity,
        "asset.delete",
        Action::Delete,
        ResourceKind::Asset,
        &asset_id,
        &json!({"target_id": asset_id}),
    )
    .await?;
    let approval_for_audit = approval.as_ref().map(|pending| pending.approval.clone());
    let principals = authorization_principal_ids(&identity, principal_id);
    let mutation = if let Some(pending) = approval {
        let audit_approval = pending.approval.clone();
        let mutation_service = state.service.clone();
        let mutation_space_id = space_id.clone();
        let mutation_asset_id = asset_id.clone();
        let mutation_principals = principals.clone();
        let result = execute_approved_mutation(&state, &space_id, &identity, pending, move |_| {
            Box::pin(async move {
                mutation_service
                    .delete_asset_with_principals(
                        &mutation_space_id,
                        &mutation_asset_id,
                        &mutation_principals,
                    )
                    .await
            })
        })
        .await;
        let (_, mutation) = match result {
            Ok(value) => value,
            Err(error) => {
                let phase = human_approval_failure_phase(&error);
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(&audit_approval),
                    Some(principal_id),
                    phase,
                    "error",
                    "error",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
                return Err(if error.to_string().starts_with("human approval ") {
                    invalid_human_approval_credential()
                } else {
                    approval_binding_error(error)
                });
            }
        };
        mutation
    } else {
        let mutation_state = state.clone();
        let mutation_identity = identity.clone();
        let mutation_space_id = space_id.clone();
        let mutation_asset_id = asset_id.clone();
        with_authorized_service_mutation(
            &state,
            &space_id,
            &identity,
            Action::Delete,
            Some(ResourceRef {
                kind: ResourceKind::Asset,
                id: asset_id.clone(),
                parent: None,
            }),
            |_principal_id, principals| async move {
                with_active_request_credential(&mutation_state, &mutation_identity, || async {
                    mutation_state
                        .service
                        .delete_asset_with_principals(
                            &mutation_space_id,
                            &mutation_asset_id,
                            &principals,
                        )
                        .await
                })
                .await
                .map_err(ApiError::from_core)
            },
        )
        .await?;
        return Ok(Json(json!({"id": asset_id, "status": "deleted"})));
    };
    match mutation {
        Ok(()) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_succeeded",
                    "success",
                    "success",
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
        }
        Err(error) => {
            if let Some(approval) = approval_for_audit.as_ref() {
                append_human_approval_audit_with_subject(
                    &state,
                    &space_id,
                    Some(approval),
                    Some(principal_id),
                    "mutation_failed",
                    "error",
                    mutation_outcome(&error),
                    identity.request_id,
                )
                .await
                .map_err(ApiError::from_core)?;
            }
            return Err(ApiError::from_core(error));
        }
    }
    Ok(Json(json!({"id": asset_id, "status": "deleted"})))
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
mod security_headers_tests {
    use super::*;
    use http_body_util::BodyExt as _;

    #[test]
    fn account_recovery_credential_failures_share_authentication_error() {
        for message in [
            "account is missing",
            "recovery code is invalid",
            "recovery credentials are temporarily locked",
            "recovery challenge is stale",
        ] {
            let error = recovery_aware_auth_error(anyhow::anyhow!(message));
            assert_eq!(error.status, StatusCode::UNAUTHORIZED);
            assert_eq!(error.detail["code"], "AUTHENTICATION_FAILED");
            assert_eq!(error.detail["message"], "Authentication failed");
        }
    }

    #[test]
    fn req_sec_002_omits_hsts_for_local_origins() {
        for origin in [
            "http://localhost:8000",
            "https://localhost:8000",
            "https://127.0.0.1:8000",
            "https://[::1]:8000",
        ] {
            assert!(
                !SecurityHeadersPolicy::from_public_origin(origin).hsts,
                "local origin must not receive HSTS: {origin}"
            );
        }
        assert!(SecurityHeadersPolicy::from_public_origin("https://ugoite.example").hsts);
    }

    #[test]
    fn req_sec_002_adds_the_common_header_contract() {
        let mut headers = HeaderMap::new();
        SecurityHeadersPolicy { hsts: true }.apply(&mut headers);

        assert_eq!(
            headers.get("content-security-policy").unwrap(),
            SECURITY_HEADERS_CSP
        );
        assert!(SECURITY_HEADERS_CSP.contains("script-src 'self'"));
        assert!(!SECURITY_HEADERS_CSP.contains("script-src 'unsafe-inline'"));
        assert!(SECURITY_HEADERS_CSP.contains("img-src 'self' blob:"));
        assert!(SECURITY_HEADERS_CSP.contains("frame-src 'self' blob:"));
        assert!(SECURITY_HEADERS_CSP.contains("media-src 'self' blob:"));
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(
            headers.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(
            headers.get("permissions-policy").unwrap(),
            "camera=(), microphone=(), geolocation=()"
        );
        assert_eq!(
            headers.get("strict-transport-security").unwrap(),
            HSTS_VALUE
        );
    }

    #[test]
    fn req_int_003_selects_scope_from_the_original_uri() {
        assert_eq!(
            response_signing_scope(&Uri::from_static("/health")),
            ResponseSigningScope::Default
        );
        assert_eq!(
            response_signing_scope(&Uri::from_static("/api/health")),
            ResponseSigningScope::Default
        );
        assert_eq!(
            response_signing_scope(&Uri::from_static("/api/spaces/encoded%2Dspace/forms")),
            ResponseSigningScope::Space("encoded-space".to_owned())
        );
        assert_eq!(
            response_signing_scope(&Uri::from_static("/mcp")),
            ResponseSigningScope::Default
        );
        assert_eq!(
            response_signing_scope(&Uri::from_static("/api/spaces/%2F/forms")),
            ResponseSigningScope::Unsigned
        );
        assert_eq!(
            response_signing_scope(&Uri::from_static("/api/spaces/%ZZ/forms")),
            ResponseSigningScope::Unsigned
        );
    }

    #[test]
    fn req_int_003_marks_only_bounded_in_memory_responses_signable() {
        let response = Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert!(signable_response_body_size(&response).is_some());

        let event_stream = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from("data: {}\n\n"))
            .unwrap();
        assert!(signable_response_body_size(&event_stream).is_none());

        let with_trailer = Response::builder()
            .header(header::TRAILER, "x-checksum")
            .body(Body::from("{}"))
            .unwrap();
        assert!(signable_response_body_size(&with_trailer).is_none());

        let oversized = Response::builder()
            .body(Body::from(vec![0; MAX_SIGNED_RESPONSE_BYTES + 1]))
            .unwrap();
        assert!(signable_response_body_size(&oversized).is_none());

        let mut explicitly_unsigned = Response::builder().body(Body::from("{}")).unwrap();
        explicitly_unsigned
            .extensions_mut()
            .insert(UnsignedResponse);
        assert!(signable_response_body_size(&explicitly_unsigned).is_none());
    }

    #[tokio::test]
    async fn response_body_collection_replays_prefix_before_failure() {
        let body = Body::from_stream(stream::iter([
            Ok::<Bytes, axum::Error>(Bytes::from("prefix")),
            Err(axum::Error::new(std::io::Error::other("body failed"))),
        ]));

        let replayed = collect_response_body_preserving_failure(body, 1024)
            .await
            .expect_err("fallible body must remain unsigned");
        let mut stream = replayed.into_data_stream();
        assert_eq!(stream.next().await.unwrap().unwrap(), Bytes::from("prefix"));
        assert!(stream.next().await.unwrap().is_err());
    }

    #[tokio::test]
    async fn response_body_collection_preserves_actual_trailers_without_signing() {
        let mut trailers = HeaderMap::new();
        trailers.insert("x-checksum", HeaderValue::from_static("abc"));
        let body = http_body_util::Full::from(Bytes::from("body")).with_trailers(
            std::future::ready(Some(Ok::<_, std::convert::Infallible>(trailers.clone()))),
        );

        let replayed = collect_response_body_preserving_failure(Body::new(body), 1024)
            .await
            .expect_err("trailer-bearing bodies must remain unsigned");
        let mut frames = http_body_util::BodyStream::new(replayed);
        assert_eq!(
            frames.next().await.unwrap().unwrap().into_data().unwrap(),
            Bytes::from("body")
        );
        assert_eq!(
            frames
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_trailers()
                .unwrap(),
            trailers
        );
    }
}

#[cfg(test)]
mod oidc_integration_tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use p256::ecdsa::{signature::Signer, SigningKey};
    use std::collections::HashMap;
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct MockIssuer {
        issuer: String,
        signing_key: SigningKey,
        subject: String,
    }

    async fn mock_configuration(State(mock): State<MockIssuer>) -> Json<Value> {
        Json(json!({
            "issuer": mock.issuer,
            "authorization_endpoint": format!("{}/authorize", mock.issuer),
            "token_endpoint": format!("{}/token", mock.issuer),
            "jwks_uri": format!("{}/jwks", mock.issuer),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["ES256"]
        }))
    }

    #[derive(Deserialize)]
    struct MockAuthorizeQuery {
        redirect_uri: String,
        state: String,
        nonce: String,
    }

    async fn mock_authorize(Query(query): Query<MockAuthorizeQuery>) -> Redirect {
        let mut redirect = url::Url::parse(&query.redirect_uri).expect("mock redirect URI");
        redirect
            .query_pairs_mut()
            .append_pair("code", &query.nonce)
            .append_pair("state", &query.state);
        Redirect::temporary(redirect.as_str())
    }

    async fn mock_token(
        State(mock): State<MockIssuer>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        let nonce = form.get("code").cloned().unwrap_or_default();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","kid":"mock-key","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "iss": mock.issuer,
                "sub": mock.subject,
                "aud": "client",
                "exp": Utc::now().timestamp() + 300,
                "iat": Utc::now().timestamp(),
                "nonce": nonce
            }))
            .expect("mock ID Token claims"),
        );
        let signing_input = format!("{header}.{payload}");
        let signature: p256::ecdsa::Signature = mock.signing_key.sign(signing_input.as_bytes());
        let token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        );
        Json(json!({
            "access_token": "mock-upstream-access-token",
            "token_type": "Bearer",
            "id_token": token
        }))
    }

    async fn mock_jwks(State(mock): State<MockIssuer>) -> Json<Value> {
        let point = mock.signing_key.verifying_key().to_encoded_point(false);
        Json(json!({
            "keys": [{
                "kty": "EC",
                "crv": "P-256",
                "x": URL_SAFE_NO_PAD.encode(point.x().expect("mock key x")),
                "y": URL_SAFE_NO_PAD.encode(point.y().expect("mock key y")),
                "use": "sig",
                "kid": "mock-key",
                "alg": "ES256"
            }]
        }))
    }

    async fn start_mock_issuer(
        subject: &str,
    ) -> anyhow::Result<(MockIssuer, tokio::task::JoinHandle<()>)> {
        let signing_key = SigningKey::from_bytes((&[7_u8; 32]).into())?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let issuer = format!("http://{}", listener.local_addr()?);
        let mock = MockIssuer {
            issuer,
            signing_key,
            subject: subject.to_string(),
        };
        let router = Router::new()
            .route("/.well-known/openid-configuration", get(mock_configuration))
            .route("/authorize", get(mock_authorize))
            .route("/token", post(mock_token))
            .route("/jwks", get(mock_jwks))
            .with_state(mock.clone());
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok((mock, handle))
    }

    #[tokio::test]
    async fn oidc_mock_issuer_completes_invitation_login_with_pkce_and_federated_session(
    ) -> anyhow::Result<()> {
        let state =
            AppState::new_for_tests(format!("memory://server-oidc-mock-{}", Uuid::now_v7()))?;
        state.initialize_node().await?;
        let actor_id = Uuid::now_v7();
        state
            .identity
            .seed_test_recovery_accounts(&[(actor_id, Uuid::now_v7(), Uuid::now_v7())])
            .await?;
        let (_invitation, invitation_token) = state
            .identity
            .issue_invitation(actor_id, "Mock invited account", None, None)
            .await?;
        let (mock, server) = start_mock_issuer("mock-subject").await?;
        let provider_id = Uuid::now_v7();
        state
            .identity
            .seed_test_oidc_provider(ugoite_identity::node_identity::OidcProvider {
                provider_id,
                issuer: mock.issuer.clone(),
                client_id: "client".to_string(),
                client_secret: None,
                enabled: true,
                created_at: Utc::now().to_rfc3339(),
            })
            .await?;

        let (redirect, state_hash) =
            start_oidc_authorization(&state, provider_id, Some(&invitation_token), None, None)
                .await
                .map_err(|error| anyhow::anyhow!("OIDC start failed: {error:?}"))?;
        let redirect_response = redirect.into_response();
        let location = redirect_response
            .headers()
            .get(header::LOCATION)
            .expect("OIDC redirect")
            .to_str()?;
        let state_token = url::Url::parse(location)?
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .expect("OIDC state");
        let attempt = state
            .identity
            .inspect_test_oidc_attempt(&state_token)
            .await?;
        let authorization = mock_authorize(Query(MockAuthorizeQuery {
            redirect_uri: url::Url::parse(location)?
                .query_pairs()
                .find(|(key, _)| key == "redirect_uri")
                .map(|(_, value)| value.into_owned())
                .expect("OIDC redirect URI"),
            state: state_token.clone(),
            nonce: attempt.nonce.clone(),
        }))
        .await;
        let authorization_response = authorization.into_response();
        let authorization_location = authorization_response
            .headers()
            .get(header::LOCATION)
            .expect("mock authorization callback")
            .to_str()?;
        let callback_url = url::Url::parse(authorization_location)?;
        let callback_code = callback_url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .expect("mock authorization code");
        let callback_state = callback_url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .expect("mock authorization state");
        let mut wrong_state_headers = HeaderMap::new();
        wrong_state_headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!(
                "{OIDC_STATE_COOKIE}={}",
                hex::encode(Sha256::digest(b"wrong-state"))
            ))?,
        );
        assert!(oidc_callback(
            State(state.clone()),
            wrong_state_headers,
            Query(OidcCallbackQuery {
                code: Some(callback_code.clone()),
                state: callback_state.clone(),
                error: None,
            }),
        )
        .await
        .is_err());
        let unknown_state = "not-the-saved-state";
        let mut unknown_state_headers = HeaderMap::new();
        unknown_state_headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!(
                "{OIDC_STATE_COOKIE}={}",
                hex::encode(Sha256::digest(unknown_state.as_bytes()))
            ))?,
        );
        assert!(oidc_callback(
            State(state.clone()),
            unknown_state_headers,
            Query(OidcCallbackQuery {
                code: Some(callback_code.clone()),
                state: unknown_state.to_string(),
                error: None,
            }),
        )
        .await
        .is_err());
        let callback = oidc_callback(
            State(state.clone()),
            [(
                header::COOKIE,
                HeaderValue::from_str(&format!("{OIDC_STATE_COOKIE}={state_hash}"))?,
            )]
            .into_iter()
            .collect(),
            Query(OidcCallbackQuery {
                code: Some(callback_code),
                state: callback_state,
                error: None,
            }),
        )
        .await
        .map_err(|error| anyhow::anyhow!("OIDC callback failed: {error:?}"))?;
        assert_eq!(callback.status(), StatusCode::SEE_OTHER);
        let cookie = callback
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|value| {
                value
                    .to_str()
                    .ok()
                    .filter(|value| value.starts_with("ugoite_session="))
            })
            .expect("Federated browser session");
        let session_token = cookie
            .strip_prefix("ugoite_session=")
            .and_then(|value| value.split(';').next())
            .expect("opaque session cookie");
        let account = state
            .identity
            .list_accounts()
            .await?
            .into_iter()
            .find(|account| account.display_name == "Mock invited account")
            .expect("invited OIDC account");
        let session = state.identity.authenticate_session(session_token).await?;
        assert_eq!(session.account.account_id, account.account_id);
        assert!(matches!(session.assurance, AssuranceLevel::Federated));
        assert!(session.passkey_bootstrap);
        assert_eq!(
            state
                .identity
                .list_sessions(account.account_id)
                .await?
                .len(),
            1
        );
        assert!(state
            .identity
            .list_oidc_links(account.account_id)
            .await?
            .iter()
            .all(|link| link.get("issuer").and_then(Value::as_str) == Some(mock.issuer.as_str())));
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn oidc_provider_configuration_rejects_non_https_issuer() -> anyhow::Result<()> {
        assert!(normalized_oidc_issuer("https://issuer.example?tenant=internal").is_err());
        assert!(
            validate_oidc_endpoint(&url::Url::parse("http://issuer.example/token")?, "token")
                .is_err()
        );
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        service
            .seed_test_recovery_accounts(&[(account_id, Uuid::now_v7(), Uuid::now_v7())])
            .await?;
        assert!(service
            .configure_oidc_provider(account_id, "http://issuer.example", "client", None)
            .await
            .is_err());
        Ok(())
    }
}

#[cfg(test)]
mod human_approval_tests {
    use super::*;

    #[test]
    fn approval_request_derives_operation_specific_delete_bindings() {
        let (action, resource, intent) = approval_request_binding(
            "entry.delete",
            &json!({"target_id": "entry-1", "hard_delete": false}),
        )
        .expect("valid delete mutation");
        assert_eq!(action, Action::Delete);
        assert_eq!(resource.kind, ResourceKind::Entry);
        assert_eq!(resource.id, "entry-1");
        assert_eq!(intent["hard_delete"], false);
        assert!(approval_request_binding(
            "entry.delete",
            &json!({"target_id": "entry-1", "hard_delete": true}),
        )
        .is_err());
        for (operation, kind) in [
            ("sql.delete", ResourceKind::SavedSql),
            ("asset.delete", ResourceKind::Asset),
        ] {
            let (action, resource, intent) =
                approval_request_binding(operation, &json!({"target_id": "resource-1"}))
                    .expect("valid non-entry delete mutation");
            assert_eq!(action, Action::Delete);
            assert_eq!(resource.kind, kind);
            assert_eq!(intent, json!({"target_id": "resource-1"}));
            assert!(approval_request_binding(
                operation,
                &json!({"target_id": "resource-1", "hard_delete": false}),
            )
            .is_err());
        }
    }

    #[test]
    fn approval_input_resource_validation_is_unprocessable() {
        let error = approval_request_binding("asset.delete", &json!({"target_id": "bad id"}))
            .expect_err("invalid resource id");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn approval_request_binds_the_complete_access_policy() {
        let policy_id = Uuid::now_v7();
        let mutation = json!({
            "kind": "entry",
            "resource_id": "entry-1",
            "policy": {
                "policy_id": policy_id,
                "inherit_space_role": false,
                "grants": []
            }
        });
        let (action, resource, intent) =
            approval_request_binding("access.put", &mutation).expect("valid policy mutation");
        assert_eq!(action, Action::Share);
        assert_eq!(resource.kind, ResourceKind::Entry);
        assert_eq!(resource.id, "entry-1");
        assert_eq!(intent, canonical_json(&mutation));
        assert!(approval_request_binding(
            "access.put",
            &json!({"kind": "entry", "resource_id": "entry-1", "policy": {}}),
        )
        .is_err());
    }

    #[test]
    fn approval_rejection_audit_has_no_secret_or_fake_target() {
        let subject = Uuid::now_v7();
        let (_, event) = human_approval_audit_event(
            "demo",
            None,
            Some(subject),
            "rejected",
            "error",
            "error",
            Uuid::now_v7(),
        );
        assert_eq!(event["subject_principal_id"], json!(subject));
        assert!(event["target_id"].is_null());
        assert!(event["metadata"]["approval_id"].is_null());
        assert!(!event.to_string().contains("token"));
    }

    #[test]
    fn approval_error_codes_keep_security_precedence_stable() {
        assert_eq!(
            approval_binding_error(AppError::forbidden("HUMAN_APPROVAL_INVALID").into()).status,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            approval_binding_error(
                AppError::expired(ErrorCode::InvalidInput, "HUMAN_APPROVAL_EXPIRED").into()
            )
            .status,
            StatusCode::GONE
        );
        assert_eq!(
            approval_binding_error(
                AppError::conflict(ErrorCode::InvalidInput, "HUMAN_APPROVAL_REPLAYED").into()
            )
            .status,
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn approval_issue_payload_rejects_unknown_fields_like_openapi() {
        let result = serde_json::from_value::<HumanApprovalIssuePayload>(json!({
            "operation": "asset.delete",
            "mutation": {"target_id": "asset-1"},
            "actor_credential_id": Uuid::now_v7(),
            "expires_in_seconds": 30,
            "action": "delete"
        }));
        assert!(result.is_err(), "unknown approval fields must fail closed");
    }
}

#[cfg(test)]
mod authentication_regression_tests {
    use super::*;
    use axum::{
        body::Body,
        http::Request,
        routing::{delete, get, post, put},
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use p256::ecdsa::{signature::Signer, Signature, SigningKey};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    use ugoite_identity::node_identity::{token_hash, RecoveryAuditOutboxRecord};

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
            session_token: None,
            human_approval_token: None,
            human_approval_header_invalid: false,
            request_id: Uuid::now_v7(),
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
    fn agent_token_audience_preserves_legacy_tokens_and_binds_mcp_tokens() {
        let issuer = "https://ugoite.example";
        assert_eq!(agent_token_audience(issuer, None), issuer);
        assert_eq!(
            agent_token_audience(issuer, Some("https://ugoite.example/mcp")),
            "https://ugoite.example/mcp"
        );
    }

    #[test]
    fn agent_token_claims_preserve_autonomous_and_delegated_subjects() {
        let agent = Uuid::now_v7();
        let human = Uuid::now_v7();
        let credential = Uuid::now_v7();
        let space = Uuid::now_v7();
        let jwk = json!({"kty":"EC","crv":"P-256","x":"x","y":"y"});
        let autonomous = build_agent_token_claims(
            "https://ugoite.example",
            Uuid::now_v7(),
            agent,
            credential,
            &jwk,
            ["read".to_string()].into_iter().collect(),
            vec![agent],
            space,
            None,
            None,
        )
        .expect("autonomous claims");
        assert_eq!(autonomous.sub, agent);
        assert_eq!(autonomous.principal_type, "agent");
        assert_eq!(autonomous.actor_principal_id, Some(agent));
        assert_eq!(autonomous.aud, "https://ugoite.example");

        let delegated = build_agent_token_claims(
            "https://ugoite.example",
            Uuid::now_v7(),
            agent,
            credential,
            &jwk,
            ["read".to_string()].into_iter().collect(),
            vec![agent, human],
            space,
            Some(human),
            Some("https://ugoite.example/mcp"),
        )
        .expect("delegated claims");
        assert_eq!(delegated.sub, human);
        assert_eq!(delegated.principal_type, "human");
        assert_eq!(delegated.actor_principal_id, Some(agent));
        assert_eq!(delegated.aud, "https://ugoite.example/mcp");
    }

    async fn call_oauth_token(state: &AppState, payload: Value) -> ApiResult<Json<Value>> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        oauth_token(
            State(state.clone()),
            headers,
            Request::post("/oauth/token")
                .body(Body::from(payload.to_string()))
                .expect("OAuth request"),
        )
        .await
    }

    fn assert_invalid_target(result: ApiResult<Json<Value>>) {
        let error = result.expect_err("resource mismatch must fail");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.detail, json!({"error": "invalid_target"}));
    }

    fn agent_test_key() -> (SigningKey, Value) {
        let key = SigningKey::from_bytes((&[7_u8; 32]).into()).expect("agent signing key");
        let point = key.verifying_key().to_encoded_point(false);
        let jwk = json!({
            "kty": "EC",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode(point.x().expect("x")),
            "y": URL_SAFE_NO_PAD.encode(point.y().expect("y")),
        });
        (key, jwk)
    }

    fn agent_test_assertion(key: &SigningKey, jwk: &Value, jti: Uuid) -> String {
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"alg":"ES256","typ":"JWT","jwk":jwk})).expect("JWT header"),
        );
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "iss": "mcp-test-client",
                "sub": "mcp-test-client",
                "aud": "http://localhost:8000/oauth/agent/token",
                "iat": chrono::Utc::now().timestamp(),
                "exp": chrono::Utc::now().timestamp() + 300,
                "jti": jti,
            }))
            .expect("JWT payload"),
        );
        let signing_input = format!("{header}.{payload}");
        let signature: Signature = key.sign(signing_input.as_bytes());
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    fn mcp_request_at_uri(
        uri: &str,
        token: &str,
        scheme: &str,
        method: &str,
        name: Option<&str>,
        params: Value,
        dpop: Option<&str>,
    ) -> Request<Body> {
        let mut params = params
            .as_object()
            .cloned()
            .expect("MCP test params are objects");
        let mut meta = params
            .remove("_meta")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        meta.entry("io.modelcontextprotocol/protocolVersion".to_string())
            .or_insert_with(|| json!("2026-07-28"));
        meta.entry("io.modelcontextprotocol/clientCapabilities".to_string())
            .or_insert_with(|| json!({}));
        params.insert("_meta".to_string(), Value::Object(meta));
        let mut builder = Request::post(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", method)
            .header(header::AUTHORIZATION, format!("{scheme} {token}"));
        if let Some(name) = name {
            builder = builder.header("mcp-name", name);
        }
        if let Some(proof) = dpop {
            builder = builder.header("dpop", proof);
        }
        builder
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
            ))
            .expect("MCP test request")
    }

    fn mcp_request(
        token: &str,
        scheme: &str,
        method: &str,
        name: Option<&str>,
        params: Value,
        dpop: Option<&str>,
    ) -> Request<Body> {
        mcp_request_at_uri("/mcp", token, scheme, method, name, params, dpop)
    }

    async fn mcp_call(
        router: Router,
        token: &str,
        scheme: &str,
        method: &str,
        name: Option<&str>,
        params: Value,
        dpop: Option<&str>,
    ) -> (StatusCode, Value) {
        let response = router
            .oneshot(mcp_request(token, scheme, method, name, params, dpop))
            .await
            .expect("MCP request response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .expect("MCP response body");
        (
            status,
            serde_json::from_slice(&body).expect("MCP JSON response"),
        )
    }

    fn mcp_dpop_proof(key: &SigningKey, jwk: &Value, token: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"alg":"ES256","typ":"dpop+jwt","jwk":jwk}))
                .expect("DPoP header"),
        );
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "htm": "POST",
                "htu": "http://localhost:8000/mcp",
                "ath": URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes())),
                "iat": chrono::Utc::now().timestamp(),
                "jti": Uuid::now_v7()
            }))
            .expect("DPoP payload"),
        );
        let signing_input = format!("{header}.{payload}");
        let signature: Signature = key.sign(signing_input.as_bytes());
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    #[tokio::test]
    async fn authenticated_mcp_route_enforces_binding_dpop_acl_and_human_mutations(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-mcp-authenticated-route-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let owner = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("mcp-authenticated-route", owner, "MCP owner")
            .await?;
        state
            .identity
            .seed_test_recovery_accounts(&[(owner, space_uid, owner)])
            .await?;
        let (signing_key, jwk) = agent_test_key();
        let actions = ["read", "create", "update", "delete"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let resource = "http://localhost:8000/mcp".to_string();
        let device = state
            .identity
            .start_device_authorization(
                "MCP approved human",
                jwk.clone(),
                Some(space_uid),
                actions.clone(),
                Some(resource.clone()),
            )
            .await?;
        state
            .identity
            .approve_device_authorization(
                device["user_code"].as_str().expect("user code"),
                owner,
                owner,
                space_uid,
                actions.clone(),
            )
            .await?;
        let (credential, _, _, _) = state
            .identity
            .exchange_device_code(device["device_code"].as_str().expect("device code"))
            .await?;
        let (issuer, node_id) = state.identity.issuer_metadata().await?;
        let now = chrono::Utc::now().timestamp();
        let claims = AccessTokenClaims {
            iss: issuer.clone(),
            node_id,
            sub: owner,
            principal_type: "human".to_string(),
            actor_principal_id: None,
            aud: resource,
            space_uid,
            granted_actions: actions,
            actor_chain: vec![owner],
            exp: now + 300,
            iat: now,
            jti: Uuid::now_v7(),
            credential_id: credential.credential_id,
            credential_generation: Some(credential.credential_generation),
            cnf: Confirmation {
                jkt: oauth::jwk_thumbprint(&jwk)?,
            },
        };
        let token = state
            .identity
            .issue_access_credential(claims.clone())
            .await?;
        let wrong_audience = state
            .identity
            .issue_access_credential(AccessTokenClaims {
                aud: issuer,
                ..claims.clone()
            })
            .await?;
        let router = app(state.clone());

        let (status, body) = mcp_call(
            router.clone(),
            &wrong_audience,
            "Bearer",
            "server/discover",
            None,
            json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["code"], "AUTHENTICATION_REQUIRED");

        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "Bearer",
            "tools/list",
            None,
            json!({}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tool_names = body["result"]["tools"]
            .as_array()
            .expect("tool list")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            tool_names,
            [
                "ugoite.search",
                "ugoite.save",
                "ugoite.undo",
                "ugoite.delete"
            ]
        );

        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "Bearer",
            "tools/call",
            Some("ugoite.save"),
            json!({"name":"ugoite.save","arguments":{"content":"---\nform: Entry\n---\n# MCP Created\n\n## Body\ncreated by MCP"},"_meta":{"ugoite/runId":"konase-work-1"}}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["result"]["structuredContent"]["status"], "created",
            "{body}"
        );
        let entry_id = body["result"]["structuredContent"]["id"]
            .as_str()
            .expect("created Entry id")
            .to_string();

        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "Bearer",
            "tools/call",
            Some("ugoite.save"),
            json!({"name":"ugoite.save","arguments":{"id":entry_id,"content":"---\nform: Entry\n---\n# MCP Updated\n\n## Body\nupdated by MCP"},"_meta":{"ugoite/runId":"konase-work-1"}}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["structuredContent"]["status"], "updated");

        let uri = format!("ugoite://entry/{entry_id}");
        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "Bearer",
            "resources/read",
            Some(&uri),
            json!({"uri":uri}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["result"]["contents"].is_array());

        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "Bearer",
            "tools/call",
            Some("ugoite.undo"),
            json!({"name":"ugoite.undo","arguments":{},"_meta":{"ugoite/runId":"konase-work-1"}}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["result"]["structuredContent"]["run_id"],
            "konase-work-1"
        );
        assert_eq!(
            body["result"]["structuredContent"]["reverted_change_count"],
            2
        );

        let proof = mcp_dpop_proof(&signing_key, &jwk, &token);
        let (status, body) = mcp_call(
            router.clone(),
            &token,
            "DPoP",
            "server/discover",
            None,
            json!({}),
            Some(&proof),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["supportedVersions"][0], "2026-07-28");

        // RFC 9449 htu excludes query and fragment. The same proof shape
        // must therefore authenticate an otherwise identical query-bearing
        // MCP request, while the request routing still sees the query.
        let query_proof = mcp_dpop_proof(&signing_key, &jwk, &token);
        let query_response = router
            .clone()
            .oneshot(mcp_request_at_uri(
                "/mcp?cursor=ignored",
                &token,
                "DPoP",
                "server/discover",
                None,
                json!({}),
                Some(&query_proof),
            ))
            .await?;
        assert_eq!(query_response.status(), StatusCode::OK);

        let (status, body) = mcp_call(
            router,
            &token,
            "Bearer",
            "tools/call",
            Some("ugoite.delete"),
            json!({"name":"ugoite.delete","arguments":{"id":entry_id}}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["structuredContent"]["status"], "deleted");
        Ok(())
    }

    #[tokio::test]
    async fn oauth_device_authorization_defaults_to_read_create_update() -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-cli-default-actions-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let public_key_jwk = json!({
            "kty": "EC",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode([1_u8; 32]),
            "y": URL_SAFE_NO_PAD.encode([2_u8; 32]),
        });
        let (status, Json(body)) = oauth_device_authorization(
            State(state.clone()),
            Json(DeviceAuthorizationPayload {
                device_name: "CLI default actions".to_string(),
                public_key_jwk,
                space_uid: None,
                requested_actions: BTreeSet::new(),
                resource: None,
            }),
        )
        .await
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        assert_eq!(status, StatusCode::CREATED);
        let pending = state
            .identity
            .pending_device_authorization(body["user_code"].as_str().expect("user code"))
            .await?;
        assert_eq!(
            pending.requested_actions,
            ["read", "create", "update"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        Ok(())
    }

    #[tokio::test]
    async fn recovery_audit_delivery_failure_reloads_and_reconciles_exactly_once(
    ) -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let root_uri = format!("fs://{}", root.path().display());
        let operator = ugoite_storage::operator_from_uri(&root_uri)?;
        let service = UgoiteService::from_operator(operator.clone(), root_uri.clone());
        let identity = NodeIdentityService::new_for_tests_with_operator(
            operator.clone(),
            "localhost",
            "http://localhost:8000",
        )?;
        let state = AppState {
            service,
            identity,
            security_headers: SecurityHeadersPolicy::from_public_origin("http://localhost:8000"),
        };
        state.initialize_node().await?;

        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let old_account_id = Uuid::now_v7();
        let new_account_id = Uuid::now_v7();
        let issuer_account_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("recovery-audit-reload", issuer_principal_id, "Owner")
            .await?;
        let space_id = space_uid.to_string();
        let event_id = Uuid::now_v7();
        let request_id = Uuid::now_v7();
        let credential_id = Uuid::now_v7();
        let event = json!({
            "event_id": event_id.to_string(),
            "action": "recovery.space_binding_replaced",
            "request_id": request_id.to_string(),
            "challenge_id": Uuid::now_v7().to_string(),
            "space_uid": space_uid.to_string(),
            "subject_principal_id": target_principal_id.to_string(),
            "subject_account_id": new_account_id.to_string(),
            "actor_principal_id": issuer_principal_id.to_string(),
            "actor_account_id": issuer_account_id.to_string(),
            "credential_id": credential_id.to_string(),
            "issuer_principal_id": issuer_principal_id.to_string(),
            "issuer_account_id": issuer_account_id.to_string(),
            "issuer_credential_id": credential_id.to_string(),
            "outcome": "success",
            "metadata": {
                "space_uid": space_uid.to_string(),
                "old_account_id": old_account_id.to_string(),
                "new_account_id": new_account_id.to_string(),
                "recovery_request_id": request_id.to_string(),
            },
        });
        state
            .identity
            .seed_test_recovery_audit_outbox(RecoveryAuditOutboxRecord {
                event_id,
                action: "recovery.space_binding_replaced".to_string(),
                request_id,
                space_uid,
                principal_id: target_principal_id,
                account_id: new_account_id,
                issuer_principal_id: Some(issuer_principal_id),
                issuer_account_id: Some(issuer_account_id),
                credential_id: Some(credential_id),
                actor_principal_id: Some(issuer_principal_id),
                actor_account_id: Some(issuer_account_id),
                actor_credential_id: Some(credential_id),
                status: "pending".to_string(),
                event,
            })
            .await?;

        // A conflicting event marker makes only the Space delivery fail. The
        // Node audit stage and the pending outbox state remain durable.
        let marker_path = format!("spaces/{space_id}/audit/event-ids/{event_id}.json");
        operator
            .create_dir(&format!("spaces/{space_id}/audit/event-ids/"))
            .await?;
        operator
            .write(
                &marker_path,
                serde_json::to_vec(&json!({
                    "status": "committed",
                    "event": {
                        "event_id": event_id.to_string(),
                        "action": "recovery.conflicting_event",
                        "subject_principal_id": target_principal_id.to_string(),
                    }
                }))?,
            )
            .await?;
        assert!(reconcile_recovery_audit_outbox(&state, &space_id)
            .await
            .is_err());
        let pending_after_failure = state.identity.pending_recovery_audits().await?;
        assert_eq!(
            pending_after_failure
                .iter()
                .find(|record| record.event_id == event_id)
                .map(|record| record.status.as_str()),
            Some("node")
        );

        operator.delete(&marker_path).await?;
        drop(state);
        let restarted = AppState {
            service: UgoiteService::from_operator(operator.clone(), root_uri),
            identity: NodeIdentityService::new_for_tests_with_operator(
                operator.clone(),
                "localhost",
                "http://localhost:8000",
            )?,
            security_headers: SecurityHeadersPolicy::from_public_origin("http://localhost:8000"),
        };
        restarted.initialize_node().await?;
        assert!(restarted
            .identity
            .pending_recovery_audits()
            .await?
            .iter()
            .all(|record| record.event_id != event_id));

        // A second reconciliation after restart must observe the committed
        // marker and leave one canonical Space audit event.
        reconcile_recovery_audit_outbox(&restarted, &space_id).await?;
        let audit = audit::list_audit_events(
            restarted.service.operator(),
            &space_id,
            AuditListOptions::default(),
        )
        .await?;
        assert_eq!(audit["total"], 1);
        assert_eq!(
            audit["items"][0]["action"],
            "recovery.space_binding_replaced"
        );
        Ok(())
    }

    #[tokio::test]
    async fn oauth_mcp_device_authorization_defaults_to_read_only() -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-mcp-default-actions-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let public_key_jwk = json!({
            "kty": "EC",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode([3_u8; 32]),
            "y": URL_SAFE_NO_PAD.encode([4_u8; 32]),
        });
        let (status, Json(body)) = oauth_device_authorization(
            State(state.clone()),
            Json(DeviceAuthorizationPayload {
                device_name: "MCP default actions".to_string(),
                public_key_jwk,
                space_uid: None,
                requested_actions: BTreeSet::new(),
                resource: Some("http://localhost:8000/mcp".to_string()),
            }),
        )
        .await
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        assert_eq!(status, StatusCode::CREATED);
        let pending = state
            .identity
            .pending_device_authorization(body["user_code"].as_str().expect("user code"))
            .await?;
        assert_eq!(
            pending.requested_actions,
            ["read"].into_iter().map(str::to_string).collect()
        );
        Ok(())
    }

    #[tokio::test]
    async fn oauth_resource_mismatch_preserves_authorization_code_device_and_refresh_grants(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-oauth-resource-preservation-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let account_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        state
            .identity
            .seed_test_recovery_accounts(&[(account_id, space_uid, principal_id)])
            .await?;
        let mcp_resource = "http://localhost:8000/mcp".to_string();
        let public_key = json!({
            "kty": "EC",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode([1_u8; 32]),
            "y": URL_SAFE_NO_PAD.encode([2_u8; 32]),
        });

        let code = state
            .identity
            .issue_authorization_code(
                "mcp-client",
                "https://client.example/callback",
                "challenge",
                public_key.clone(),
                account_id,
                principal_id,
                space_uid,
                ["read".to_string()].into_iter().collect(),
                Some(mcp_resource.clone()),
            )
            .await?;
        assert_invalid_target(
            call_oauth_token(
                &state,
                json!({"grant_type":"authorization_code","code":code}),
            )
            .await,
        );
        assert!(state
            .identity
            .pending_authorization_code(&code)
            .await
            .is_ok());

        let device = state
            .identity
            .start_device_authorization(
                "MCP device",
                public_key.clone(),
                Some(space_uid),
                ["read".to_string()].into_iter().collect(),
                Some(mcp_resource.clone()),
            )
            .await?;
        let device_code = device["device_code"].as_str().unwrap().to_string();
        assert_invalid_target(
            call_oauth_token(
                &state,
                json!({
                    "grant_type":"urn:ietf:params:oauth:grant-type:device_code",
                    "device_code":device_code
                }),
            )
            .await,
        );
        let device_request = state
            .identity
            .read_state()
            .await?
            .device_authorizations
            .get(&token_hash(&device_code))
            .cloned()
            .expect("device grant");
        assert!(device_request.last_polled_at.is_none());

        let refresh_source = state
            .identity
            .start_device_authorization(
                "MCP refresh device",
                public_key,
                Some(space_uid),
                ["read".to_string()].into_iter().collect(),
                Some(mcp_resource),
            )
            .await?;
        let refresh_device_code = refresh_source["device_code"].as_str().unwrap();
        let refresh_user_code = refresh_source["user_code"].as_str().unwrap();
        state
            .identity
            .approve_device_authorization(
                refresh_user_code,
                account_id,
                principal_id,
                space_uid,
                ["read".to_string()].into_iter().collect(),
            )
            .await?;
        let (_, _, refresh_token, _) = state
            .identity
            .exchange_device_code(refresh_device_code)
            .await?;
        assert_invalid_target(
            call_oauth_token(
                &state,
                json!({"grant_type":"refresh_token","refresh_token":refresh_token}),
            )
            .await,
        );
        assert!(state
            .identity
            .refresh_credential(&refresh_token)
            .await
            .is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn autonomous_and_delegated_agent_endpoints_issue_exact_mcp_audience_tokens(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-agent-resource-boundary-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let owner = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("agent-resource-boundary", owner, "Owner")
            .await?;
        state
            .identity
            .seed_test_recovery_accounts(&[(owner, space_uid, owner)])
            .await?;
        let space_id = space_uid.to_string();
        let agent = Authorizer::new(state.service.operator().clone())
            .create_agent(
                &space_id,
                owner,
                ugoite_iceberg::authorization::CreateAgentRequest {
                    display_name: "MCP resource agent".to_string(),
                    description: String::new(),
                    mode: AgentMode::Both,
                    owner_principal_ids: [owner].into_iter().collect(),
                    granted_actions: [Action::Read].into_iter().collect(),
                    expires_at: Some(
                        (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    ),
                },
            )
            .await?;
        let (key, jwk) = agent_test_key();
        let credential = state
            .identity
            .register_agent_credential(
                agent.agent_id,
                jwk.clone(),
                Some((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
            )
            .await?;
        let resource = Some("http://localhost:8000/mcp".to_string());
        let autonomous = issue_autonomous_agent_token(
            State(state.clone()),
            Json(AgentTokenPayload {
                credential_id: credential.credential_id,
                client_assertion: agent_test_assertion(&key, &jwk, Uuid::now_v7()),
                space_id: space_id.clone(),
                requested_actions: ["read".to_string()].into_iter().collect(),
                resource: resource.clone(),
            }),
        )
        .await
        .expect("autonomous MCP agent token")
        .0;
        let autonomous_claims = state
            .identity
            .resolve_access_credential(autonomous["access_token"].as_str().unwrap())
            .await?;
        assert_eq!(autonomous_claims.aud, "http://localhost:8000/mcp");
        assert_eq!(autonomous_claims.sub, agent.agent_id);
        assert_eq!(autonomous_claims.principal_type, "agent");

        let mut delegated_identity = content_identity(owner, space_uid);
        delegated_identity.token_principal_id = None;
        delegated_identity.token_space_uid = None;
        delegated_identity.token_actions = None;
        let delegated = issue_delegated_agent_token(
            State(state.clone()),
            Extension(delegated_identity),
            Path((space_id, agent.agent_id)),
            Json(AgentTokenPayload {
                credential_id: credential.credential_id,
                client_assertion: agent_test_assertion(&key, &jwk, Uuid::now_v7()),
                space_id: space_uid.to_string(),
                requested_actions: ["read".to_string()].into_iter().collect(),
                resource,
            }),
        )
        .await
        .expect("delegated MCP agent token")
        .0;
        let delegated_claims = state
            .identity
            .resolve_access_credential(delegated["access_token"].as_str().unwrap())
            .await?;
        assert_eq!(delegated_claims.aud, "http://localhost:8000/mcp");
        assert_eq!(delegated_claims.sub, owner);
        assert_eq!(delegated_claims.principal_type, "human");
        assert_eq!(delegated_claims.actor_principal_id, Some(agent.agent_id));
        Ok(())
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

    #[test]
    fn non_local_mutation_error_uses_the_stable_gateway_envelope() {
        let error = ApiError::from_core(
            AppError::dependency_unavailable(
                ErrorCode::StorageMutationUnavailable,
                "non-local Space mutations are unavailable in v0.1",
            )
            .into(),
        );
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.detail["code"], "STORAGE_MUTATION_UNAVAILABLE");
    }

    #[test]
    fn owner_recovery_ambiguous_node_commit_stays_fenced() {
        let committed = owner_recovery_commit_api_error(anyhow::anyhow!(
            "node control write committed with an ambiguous response"
        ));
        assert_eq!(committed.status, StatusCode::CONFLICT);
        assert_eq!(committed.detail["code"], "RECOVERY_FENCE_UNAVAILABLE");

        let unknown =
            owner_recovery_commit_api_error(anyhow::anyhow!("node control write outcome unknown"));
        assert_eq!(unknown.status, StatusCode::CONFLICT);
        assert_eq!(unknown.detail["code"], "RECOVERY_FENCE_UNAVAILABLE");
    }

    #[test]
    fn space_recovery_write_ambiguity_stays_fenced() {
        let committed = anyhow::anyhow!(
            "Space authorization write committed with an ambiguous response: timeout"
        );
        let unknown = anyhow::anyhow!(
            "Space authorization write outcome unknown: timeout; verification failed: unavailable"
        );
        assert!(space_write_outcome_is_ambiguous(&committed));
        assert!(space_write_outcome_is_ambiguous(&unknown));
        assert_eq!(
            recovery_reservation_error(committed).status,
            StatusCode::CONFLICT
        );
        assert_eq!(
            recovery_reservation_error(unknown).status,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn recovery_coordinator_retries_missing_space_half_and_stays_fail_closed(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-recovery-coordinator-{}",
            Uuid::now_v7()
        ))?;
        state.identity.bootstrap_if_needed().await?;
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("recovery-coordinator", issuer_principal_id, "Owner")
            .await?;
        let space_id = space_uid.to_string();
        Authorizer::new(state.service.operator().clone())
            .add_human_member(
                &space_id,
                issuer_principal_id,
                SpacePrincipal {
                    principal_id: target_principal_id,
                    kind: PrincipalKind::Human,
                    display_name: "Recovery target".into(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        state
            .identity
            .seed_test_recovery_accounts(&[
                (issuer_account_id, space_uid, issuer_principal_id),
                (target_account_id, space_uid, target_principal_id),
            ])
            .await?;

        let request_id = Uuid::now_v7();
        let fence_id = Uuid::now_v7();
        let provisional = RecoveryBindingSnapshot {
            request_id,
            recovery_fence_id: fence_id,
            recovery_fence_expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5))
                .to_rfc3339(),
            space_authorization_revision: 0,
            issuer_space_lifecycle_epoch: 0,
            target_space_lifecycle_epoch: 0,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        state
            .identity
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&provisional),
            )
            .await?;

        // A separate process can observe the Node half before the Space CAS
        // becomes visible. Reconciliation must retain the barrier rather
        // than release it based on that single missing read.
        reconcile_recovery_fences(&state, &space_id).await?;
        assert_eq!(
            state.identity.recovery_fence_status(fence_id).await?,
            Some("active".into())
        );
        assert_eq!(
            state.identity.recovery_fence_phase(fence_id).await?,
            Some("paired".into())
        );
        assert_eq!(
            Authorizer::new(state.service.operator().clone())
                .recovery_fence(&space_id, fence_id)
                .await?
                .request_id,
            request_id
        );

        // A later retry with the same durable request identity reuses the
        // exact Space fence instead of allocating a new fence.
        let (authorizer, space_fence, snapshot) = reserve_recovery_pair(
            &state,
            &space_id,
            space_uid,
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            request_id,
            chrono::Duration::minutes(5),
        )
        .await
        .expect("same-key recovery retry should converge");
        assert_eq!(space_fence.fence_id, fence_id);
        assert_eq!(snapshot.recovery_fence_id, fence_id);
        assert_eq!(
            state.identity.recovery_fence_phase(fence_id).await?,
            Some("paired".into())
        );
        assert_eq!(
            authorizer.recovery_fence(&space_id, fence_id).await?.status,
            "active"
        );

        // A terminal Space result is the only signal that permits Node
        // cleanup; this also covers the restart reconciliation path.
        authorizer
            .release_recovery_fence(&space_id, fence_id)
            .await?;
        reconcile_recovery_fences(&state, &space_id).await?;
        assert_eq!(
            state.identity.recovery_fence_status(fence_id).await?,
            Some("released".into())
        );
        Ok(())
    }

    #[tokio::test]
    async fn protected_auth_errors_use_the_openapi_error_envelope() -> anyhow::Result<()> {
        let response = unauthorized("a valid session or DPoP access token is required");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        assert_eq!(
            serde_json::from_slice::<Value>(&body)?,
            json!({
                "code": "AUTHENTICATION_REQUIRED",
                "message": "a valid session or DPoP access token is required"
            })
        );
        Ok(())
    }

    #[tokio::test]
    /// REQ-SEC-003
    async fn req_sec_003_rejects_unauthenticated_rest_requests() -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-mandatory-auth-rest-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let owner = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("mandatory-auth-rest", owner, "Owner")
            .await?;
        state
            .identity
            .seed_test_recovery_accounts(&[(owner, space_uid, owner)])
            .await?;

        let response = app(state)
            .oneshot(Request::get("/spaces").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            axum::body::to_bytes(response.into_body(), usize::MAX).await?,
            serde_json::to_vec(&json!({
                "code": "AUTHENTICATION_REQUIRED",
                "message": "a valid session or DPoP access token is required"
            }))?
        );
        Ok(())
    }

    #[tokio::test]
    /// REQ-SEC-008
    async fn req_sec_008_space_audit_is_visible_only_to_space_owners() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-space-audit-visibility")?;
        state.initialize_node().await?;
        let owner = Uuid::now_v7();
        let viewer = Uuid::now_v7();
        let space_slug = "space-audit-visibility";
        let space_uid = state
            .service
            .create_space_for_principal(space_slug, owner, "Owner")
            .await?;
        let space_id = space_uid.to_string();
        Authorizer::new(state.service.operator().clone())
            .add_human_member(
                &space_id,
                owner,
                SpacePrincipal {
                    principal_id: viewer,
                    kind: PrincipalKind::Human,
                    display_name: "Viewer".to_string(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        state
            .identity
            .bind_local_owner(space_uid, owner, owner)
            .await?;
        state
            .identity
            .bind_local_owner(space_uid, viewer, viewer)
            .await?;
        audit::append_audit_event(
            state.service.operator(),
            &space_id,
            &json!({
                "event_id": Uuid::now_v7(),
                "action": "member.revoked",
                "subject_principal_id": viewer,
                "actor_principal_id": owner,
                "outcome": "success"
            }),
            None,
        )
        .await?;

        let audit_actions = Some(
            ["read", "create", "update", "share"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        let mut owner_identity = content_identity(owner, space_uid);
        owner_identity.token_principal_id = None;
        owner_identity.token_space_uid = None;
        owner_identity.token_actions = None;
        let owner_route = Router::new()
            .route("/spaces/{space_id}/audit", get(list_audit_events))
            .layer(Extension(owner_identity))
            .with_state(state.clone());
        let owner_response = owner_route
            .oneshot(Request::get(format!("/spaces/{space_id}/audit")).body(Body::empty())?)
            .await?;
        assert_eq!(owner_response.status(), StatusCode::OK);
        let owner_body = axum::body::to_bytes(owner_response.into_body(), usize::MAX).await?;
        let owner_body: Value = serde_json::from_slice(&owner_body)?;
        assert!(owner_body["total"].as_u64().is_some_and(|total| total >= 1));
        assert!(owner_body["items"].as_array().is_some_and(|items| {
            items.iter().any(|event| {
                event["action"] == "member.revoked" && event["actor_principal_id"] == json!(owner)
            })
        }));

        let mut viewer_identity = content_identity(viewer, space_uid);
        viewer_identity.token_principal_id = None;
        viewer_identity.token_space_uid = None;
        viewer_identity.token_actions = audit_actions;
        let viewer_route = Router::new()
            .route("/spaces/{space_id}/audit", get(list_audit_events))
            .layer(Extension(viewer_identity))
            .with_state(state);
        let viewer_response = viewer_route
            .oneshot(Request::get(format!("/spaces/{space_id}/audit")).body(Body::empty())?)
            .await?;
        assert_eq!(viewer_response.status(), StatusCode::FORBIDDEN);
        Ok(())
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
    /// REQ-FORM-009
    async fn delete_route_records_the_authenticated_principal_as_actor() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-entry-attribution-delete")?;
        let principal_id = Uuid::from_u128(1918);
        let space_id = state
            .service
            .create_space_for_principal("entry-attribution-delete", principal_id, "Route test")
            .await?
            .to_string();
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "name": "Entry",
                    "fields": {"Body": {"type": "markdown"}},
                    "allow_extra_attributes": "deny"
                }),
            )
            .await?;
        state
            .service
            .create_entry(
                &space_id,
                "attribution-delete-entry",
                "---\nform: Entry\n---\n# Created\n\n## Body\nBody",
                "creator",
            )
            .await?;
        let space_uid = state.service.space_uid(&space_id).await?;
        state.identity.bootstrap_if_needed().await?;
        state
            .identity
            .bind_local_owner(space_uid, principal_id, principal_id)
            .await?;
        let agent_credential = state
            .identity
            .register_agent_credential(principal_id, json!({}), None)
            .await?;
        let mut identity = content_identity(principal_id, space_uid);
        identity.request_identity.credential_id = agent_credential.credential_id;
        identity.request_identity.authentication_method =
            RequestAuthenticationMethod::AgentAssertion;
        identity.token_principal_id = None;
        identity.token_space_uid = None;
        identity.token_actions = None;
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state.clone());

        let response = route
            .oneshot(
                Request::delete(format!(
                    "/spaces/{space_id}/entries/attribution-delete-entry"
                ))
                .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        let history = state
            .service
            .entry_history(&space_id, "attribution-delete-entry")
            .await?;
        let delete_revision = history["revisions"]
            .as_array()
            .and_then(|revisions| revisions.last())
            .expect("delete revision");
        assert_eq!(delete_revision["author"], "creator");
        assert_eq!(delete_revision["updated_by"], principal_id.to_string());
        assert_eq!(delete_revision["deleted_by"], principal_id.to_string());
        Ok(())
    }

    #[tokio::test]
    async fn sql_and_asset_delete_routes_bind_approval_without_entry_hard_delete(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-delete-routes")?;
        let principal_id = Uuid::from_u128(1926);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-delete-routes", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let mut identity = content_identity(principal_id, space_uid);
        identity.token_principal_id = Some(principal_id);
        identity.token_space_uid = Some(space_uid);
        identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        identity.human_approval_token = Some("A".repeat(43));
        let route = Router::new()
            .route("/spaces/{space_id}/sql/{sql_id}", delete(delete_sql))
            .route("/spaces/{space_id}/assets/{asset_id}", delete(delete_asset))
            .layer(Extension(identity))
            .with_state(state);

        for uri in [
            format!("/spaces/{space_id}/sql/{}", Uuid::now_v7()),
            format!("/spaces/{space_id}/assets/{}", Uuid::now_v7()),
        ] {
            let response = route
                .clone()
                .oneshot(Request::delete(uri).body(Body::empty())?)
                .await?;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
            let body: Value = serde_json::from_slice(&body)?;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "unexpected approval response: {body}"
            );
            assert_eq!(body["code"], "HUMAN_APPROVAL_INVALID");
        }
        Ok(())
    }

    #[tokio::test]
    async fn access_policy_route_returns_json_for_malformed_json() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-json-rejection")?;
        let principal_id = Uuid::from_u128(1930);
        let space_uid = Uuid::now_v7();
        let route = Router::new()
            .route(
                "/spaces/{space_id}/policies/{kind}/{resource_id}",
                axum::routing::put(put_access_policy),
            )
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state);

        let response = route
            .oneshot(
                Request::put("/spaces/demo/policies/entry/entry-1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{"))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body: Value = serde_json::from_slice(&body)?;
        assert_eq!(body["code"], "INVALID_INPUT");
        assert!(body["message"]
            .as_str()
            .is_some_and(|message| message.contains("Failed to parse the request body as JSON")));
        Ok(())
    }

    #[tokio::test]
    async fn cross_kind_delete_approval_is_rejected_without_consuming_it() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-cross-kind")?;
        let principal_id = Uuid::from_u128(1950);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-cross-kind", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let actor_credential_id = Uuid::now_v7();
        let target_id = Uuid::now_v7().to_string();
        let (asset_approval_id, asset_token) = issue_route_test_delete_approval(
            &state,
            &space_id,
            principal_id,
            actor_credential_id,
            "asset.delete",
            ResourceKind::Asset,
            target_id.clone(),
        )
        .await?;
        let (sql_approval_id, sql_token) = issue_route_test_delete_approval(
            &state,
            &space_id,
            principal_id,
            actor_credential_id,
            "sql.delete",
            ResourceKind::SavedSql,
            target_id.clone(),
        )
        .await?;

        let mut sql_identity = content_identity(principal_id, space_uid);
        sql_identity.request_identity.credential_id = actor_credential_id;
        sql_identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        sql_identity.human_approval_token = Some(asset_token);
        let sql_route = Router::new()
            .route("/spaces/{space_id}/sql/{sql_id}", delete(delete_sql))
            .layer(Extension(sql_identity))
            .with_state(state.clone());
        let sql_response = sql_route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/sql/{target_id}"))
                    .body(Body::empty())?,
            )
            .await?;
        let sql_body = axum::body::to_bytes(sql_response.into_body(), usize::MAX).await?;
        let sql_body: Value = serde_json::from_slice(&sql_body)?;
        assert_eq!(sql_body["code"], "HUMAN_APPROVAL_INVALID");

        let mut asset_identity = content_identity(principal_id, space_uid);
        asset_identity.request_identity.credential_id = actor_credential_id;
        asset_identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        asset_identity.human_approval_token = Some(sql_token);
        let asset_route = Router::new()
            .route("/spaces/{space_id}/assets/{asset_id}", delete(delete_asset))
            .layer(Extension(asset_identity))
            .with_state(state.clone());
        let asset_response = asset_route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/assets/{target_id}"))
                    .body(Body::empty())?,
            )
            .await?;
        let asset_body = axum::body::to_bytes(asset_response.into_body(), usize::MAX).await?;
        let asset_body: Value = serde_json::from_slice(&asset_body)?;
        assert_eq!(asset_body["code"], "HUMAN_APPROVAL_INVALID");

        let authorizer = Authorizer::new(state.service.operator().clone());
        let approvals = authorizer.state(&space_id).await?.human_approvals;
        assert!(approvals
            .get(&asset_approval_id)
            .is_some_and(|approval| approval.consumed_at.is_none()));
        assert!(approvals
            .get(&sql_approval_id)
            .is_some_and(|approval| approval.consumed_at.is_none()));
        Ok(())
    }

    #[tokio::test]
    async fn dangerous_route_audits_acl_refusal_before_approval_processing() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-route-required")?;
        let principal_id = Uuid::from_u128(1927);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-route-required", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let mut identity = content_identity(principal_id, space_uid);
        identity.token_principal_id = Some(Uuid::from_u128(1928));
        identity.token_space_uid = Some(space_uid);
        identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state.clone());

        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/{}", Uuid::now_v7()))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let events = audit::list_audit_events(
            state.service.operator(),
            &space_id,
            AuditListOptions {
                action: Some("human_approval.rejected".to_string()),
                ..Default::default()
            },
        )
        .await?;
        let event = events["items"]
            .as_array()
            .and_then(|items| items.first())
            .expect("approval-required audit event");
        assert!(event["target_id"].is_null());
        assert!(event["metadata"]["approval_id"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn browser_acl_refusal_audits_the_resolved_space_principal() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-browser-required")?;
        let principal_id = Uuid::from_u128(1930);
        let space_id = state
            .service
            .create_space_for_principal(
                "human-approval-browser-required",
                principal_id,
                "Route test",
            )
            .await?
            .to_string();
        let owner_id = principal_id;
        let viewer_id = Uuid::from_u128(1931);
        Authorizer::new(state.service.operator().clone())
            .add_human_member(
                &space_id,
                owner_id,
                SpacePrincipal {
                    principal_id: viewer_id,
                    kind: PrincipalKind::Human,
                    display_name: "Viewer".into(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        let space_uid = state.service.space_uid(&space_id).await?;
        state.identity.bootstrap_if_needed().await?;
        state
            .identity
            .bind_local_owner(space_uid, viewer_id, viewer_id)
            .await?;
        let entry_id = Uuid::now_v7().to_string();
        let mut identity = content_identity(viewer_id, space_uid);
        identity.token_principal_id = None;
        identity.token_space_uid = None;
        identity.token_actions = None;
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state.clone());

        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/{entry_id}"))
                    .body(Body::empty())?,
            )
            .await?;
        let response_status = response.status();
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        assert_eq!(response_status, StatusCode::FORBIDDEN, "{response_body:?}");

        let events = audit::list_audit_events(
            state.service.operator(),
            &space_id,
            AuditListOptions {
                action: Some("human_approval.rejected".to_string()),
                ..Default::default()
            },
        )
        .await?;
        let event = events["items"]
            .as_array()
            .and_then(|items| items.first())
            .expect("browser approval-required audit event");
        assert_eq!(event["subject_principal_id"], json!(viewer_id));
        assert!(event["metadata"]["approval_id"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn browser_recent_passkey_refusal_is_audited_without_an_approval_id() -> anyhow::Result<()>
    {
        let state = AppState::new_for_tests("memory://server-human-approval-browser-passkey")?;
        let principal_id = Uuid::from_u128(1932);
        let space_id = state
            .service
            .create_space_for_principal(
                "human-approval-browser-passkey",
                principal_id,
                "Route test",
            )
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        state.identity.bootstrap_if_needed().await?;
        state
            .identity
            .bind_local_owner(space_uid, principal_id, principal_id)
            .await?;
        let mut identity = content_identity(principal_id, space_uid);
        identity.token_principal_id = None;
        identity.token_space_uid = None;
        identity.token_actions = None;
        identity.recent_passkey = false;
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state.clone());

        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/{}", Uuid::now_v7()))
                    .body(Body::empty())?,
            )
            .await?;
        let response_status = response.status();
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        assert_eq!(response_status, StatusCode::FORBIDDEN, "{response_body:?}");

        let events = audit::list_audit_events(
            state.service.operator(),
            &space_id,
            AuditListOptions {
                action: Some("human_approval.rejected".to_string()),
                ..Default::default()
            },
        )
        .await?;
        let event = events["items"]
            .as_array()
            .and_then(|items| items.first())
            .expect("browser passkey approval audit event");
        assert_eq!(event["subject_principal_id"], json!(principal_id));
        assert!(event["target_id"].is_null());
        assert!(event["metadata"]["approval_id"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn approval_issue_route_rejects_unknown_top_level_fields() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-unknown-field")?;
        let principal_id = Uuid::from_u128(1929);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-unknown-field", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let route = Router::new()
            .route("/spaces/{space_id}/approvals", post(issue_human_approval))
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state);
        let response = route
            .oneshot(
                Request::post(format!("/spaces/{space_id}/approvals"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "operation": "asset.delete",
                            "mutation": {"target_id": Uuid::now_v7()},
                            "actor_credential_id": Uuid::now_v7(),
                            "expires_in_seconds": 30,
                            "action": "delete"
                        })
                        .to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        Ok(())
    }

    #[tokio::test]
    async fn unknown_approval_token_audit_keeps_operation_attribution() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-unknown-token")?;
        let principal_id = Uuid::from_u128(1934);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-unknown-token", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let mut identity = content_identity(principal_id, space_uid);
        identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        let credential_id = identity.request_identity.credential_id;
        identity.human_approval_token = Some("X".repeat(43));
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state.clone());

        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/unknown-approval-entry"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let events = audit::list_audit_events(
            state.service.operator(),
            &space_id,
            AuditListOptions {
                action: Some("human_approval.rejected".to_string()),
                ..Default::default()
            },
        )
        .await?;
        let event = events["items"]
            .as_array()
            .and_then(|items| items.first())
            .expect("unknown-token rejection audit event");
        assert_eq!(event["actor_principal_id"], json!(principal_id));
        assert_eq!(event["credential_id"], json!(credential_id));
        assert_eq!(event["metadata"]["operation"], "entry.delete");
        assert_eq!(event["metadata"]["action"], "delete");
        assert_eq!(
            event["metadata"]["canonical_resource"],
            "entry:unknown-approval-entry"
        );
        assert!(event["metadata"]["intent_hash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64));
        Ok(())
    }

    async fn issue_route_test_approval(
        state: &AppState,
        space_id: &str,
        principal_id: Uuid,
        actor_credential_id: Uuid,
        ttl: chrono::Duration,
    ) -> anyhow::Result<(String, String)> {
        let entry_id = Uuid::from_u128(1940).to_string();
        let intent = json!({"target_id": entry_id.clone(), "hard_delete": false});
        let intent_digest = intent_hash(&intent)
            .map_err(|error| anyhow::anyhow!("invalid test intent: {}", error.detail))?;
        let authorizer = Authorizer::new(state.service.operator().clone());
        let (_, token) = authorizer
            .issue_human_approval(
                space_id,
                HumanApprovalIssue {
                    operation: "entry.delete".into(),
                    action: Action::Delete,
                    resource: ResourceRef {
                        kind: ResourceKind::Entry,
                        id: entry_id.clone(),
                        parent: None,
                    },
                    intent_hash: intent_digest,
                    actor_principal_id: principal_id,
                    actor_credential_id,
                    issuer_principal_id: principal_id,
                    issuer_account_id: principal_id,
                    issuer_credential_id: Uuid::now_v7(),
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl,
                },
            )
            .await?;
        Ok((entry_id, token))
    }

    async fn issue_route_test_delete_approval(
        state: &AppState,
        space_id: &str,
        principal_id: Uuid,
        actor_credential_id: Uuid,
        operation: &str,
        resource_kind: ResourceKind,
        resource_id: String,
    ) -> anyhow::Result<(Uuid, String)> {
        let intent = json!({"target_id": resource_id});
        let intent_digest = intent_hash(&intent)
            .map_err(|error| anyhow::anyhow!("invalid test intent: {}", error.detail))?;
        let authorizer = Authorizer::new(state.service.operator().clone());
        let (approval, token) = authorizer
            .issue_human_approval(
                space_id,
                HumanApprovalIssue {
                    operation: operation.to_string(),
                    action: Action::Delete,
                    resource: ResourceRef {
                        kind: resource_kind,
                        id: resource_id,
                        parent: None,
                    },
                    intent_hash: intent_digest,
                    actor_principal_id: principal_id,
                    actor_credential_id,
                    issuer_principal_id: principal_id,
                    issuer_account_id: principal_id,
                    issuer_credential_id: Uuid::now_v7(),
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        Ok((approval.approval_id, token))
    }

    #[tokio::test]
    async fn human_approval_issue_then_consume_uses_node_credential_lock() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-node-lock")?;
        state.initialize_node().await?;
        let issuer_account_id = Uuid::from_u128(1943);
        let issuer_principal_id = Uuid::from_u128(1944);
        let actor_account_id = Uuid::from_u128(1945);
        let actor_principal_id = Uuid::from_u128(1946);
        let issuer_credential_id = Uuid::from_u128(1947);
        let actor_credential_id = Uuid::from_u128(1948);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-node-lock", issuer_principal_id, "Owner")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let authorizer = Authorizer::new(state.service.operator().clone());
        authorizer
            .add_human_member(
                &space_id,
                issuer_principal_id,
                SpacePrincipal {
                    principal_id: actor_principal_id,
                    kind: PrincipalKind::Human,
                    display_name: "Approval actor".to_string(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Owner,
            )
            .await?;
        state
            .identity
            .seed_test_human_approval_credentials(
                space_uid,
                issuer_principal_id,
                issuer_account_id,
                actor_principal_id,
                actor_account_id,
                issuer_credential_id,
                actor_credential_id,
            )
            .await?;

        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "approval-node-lock-entry".to_string(),
            parent: None,
        };
        let intent = json!({"target_id": resource.id.clone(), "hard_delete": false});
        let intent_digest = intent_hash(&intent)
            .map_err(|error| anyhow::anyhow!("invalid test intent: {}", error.detail))?;
        let approval_request = HumanApprovalIssue {
            operation: "entry.delete".to_string(),
            action: Action::Delete,
            resource: resource.clone(),
            intent_hash: intent_digest.clone(),
            actor_principal_id,
            actor_credential_id,
            issuer_principal_id,
            issuer_account_id,
            issuer_credential_id,
            issuer_credential_generation: 0,
            issuer_node_account_lifecycle_epoch: 0,
            ttl: chrono::Duration::seconds(30),
        };
        let issue_authorizer = authorizer.clone();
        let issue_space_id = space_id.clone();
        let (approval, token) = state
            .identity
            .with_active_human_approval_issuance(
                issuer_credential_id,
                Some(issuer_account_id),
                ActiveCredentialKind::Passkey,
                issuer_account_id,
                issuer_credential_id,
                0,
                actor_credential_id,
                space_uid,
                move |issued_actor, issuer_epoch| {
                    let mut request = approval_request;
                    request.actor_principal_id = issued_actor;
                    request.issuer_node_account_lifecycle_epoch = issuer_epoch;
                    let authorizer = issue_authorizer.clone();
                    let space_id = issue_space_id.clone();
                    async move { authorizer.issue_human_approval(&space_id, request).await }
                },
            )
            .await?;

        let mut identity = content_identity(actor_principal_id, space_uid);
        identity.account_id = actor_account_id;
        identity.request_identity.subject = AuthenticatedSubject::HumanAccount {
            account_id: actor_account_id,
        };
        identity.request_identity.actor = Actor::Human {
            account_id: actor_account_id,
        };
        identity.request_identity.credential_id = actor_credential_id;
        identity.request_identity.authentication_method = RequestAuthenticationMethod::DeviceProof;
        identity.request_identity.assurance = AssuranceLevel::Possession;
        identity.token_principal_id = Some(actor_principal_id);
        identity.token_actor_principal_id = Some(actor_principal_id);
        let pending = PendingHumanApproval {
            operation: approval.operation.clone(),
            action: approval.action.clone(),
            resource: approval.resource.clone(),
            approval,
            token,
            intent_hash: intent_digest,
        };
        let (_, mutation) =
            execute_approved_mutation(&state, &space_id, &identity, pending, |_| {
                Box::pin(async { Ok::<(), anyhow::Error>(()) })
            })
            .await?;
        mutation?;

        let entry = authorizer
            .state(&space_id)
            .await?
            .human_approvals
            .values()
            .next()
            .expect("issued approval")
            .clone();
        assert!(entry.consumed_at.is_some());

        let second_intent = json!({
            "target_id": "approval-node-lock-entry-2",
            "hard_delete": false
        });
        let second_digest = intent_hash(&second_intent)
            .map_err(|error| anyhow::anyhow!("invalid second test intent: {}", error.detail))?;
        let second_authorizer = authorizer.clone();
        let second_space_id = space_id.clone();
        let second_digest_for_issue = second_digest.clone();
        let (second_approval, second_token) = state
            .identity
            .with_active_human_approval_issuance(
                issuer_credential_id,
                Some(issuer_account_id),
                ActiveCredentialKind::Passkey,
                issuer_account_id,
                issuer_credential_id,
                0,
                actor_credential_id,
                space_uid,
                move |issued_actor, issuer_epoch| {
                    let authorizer = second_authorizer.clone();
                    let space_id = second_space_id.clone();
                    let request = HumanApprovalIssue {
                        operation: "entry.delete".to_string(),
                        action: Action::Delete,
                        resource: ResourceRef {
                            kind: ResourceKind::Entry,
                            id: "approval-node-lock-entry-2".to_string(),
                            parent: None,
                        },
                        intent_hash: second_digest_for_issue.clone(),
                        actor_principal_id: issued_actor,
                        actor_credential_id,
                        issuer_principal_id,
                        issuer_account_id,
                        issuer_credential_id,
                        issuer_credential_generation: 0,
                        issuer_node_account_lifecycle_epoch: issuer_epoch,
                        ttl: chrono::Duration::seconds(30),
                    };
                    async move { authorizer.issue_human_approval(&space_id, request).await }
                },
            )
            .await?;
        state
            .identity
            .revoke_device_credential(actor_account_id, actor_credential_id)
            .await?;
        let error = execute_approved_mutation(
            &state,
            &space_id,
            &identity,
            PendingHumanApproval {
                operation: second_approval.operation.clone(),
                action: second_approval.action.clone(),
                resource: second_approval.resource.clone(),
                approval: second_approval.clone(),
                token: second_token,
                intent_hash: second_digest,
            },
            |_| Box::pin(async { Ok::<(), anyhow::Error>(()) }),
        )
        .await
        .expect_err("revoked actor device must be rejected before consume");
        assert!(error.to_string().contains("device credential"));
        assert!(authorizer
            .state(&space_id)
            .await?
            .human_approvals
            .get(&second_approval.approval_id)
            .is_some_and(|approval| approval.consumed_at.is_none()));
        Ok(())
    }

    #[tokio::test]
    async fn dangerous_route_returns_conflict_for_a_replayed_approval() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-route-replay")?;
        let principal_id = Uuid::from_u128(1941);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-route-replay", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let actor_credential_id = Uuid::now_v7();
        let (entry_id, token) = issue_route_test_approval(
            &state,
            &space_id,
            principal_id,
            actor_credential_id,
            chrono::Duration::seconds(30),
        )
        .await?;
        let authorizer = Authorizer::new(state.service.operator().clone());
        let replay_intent_hash = intent_hash(&json!({
            "target_id": entry_id.clone(),
            "hard_delete": false
        }))
        .map_err(|error| anyhow::anyhow!("invalid test intent: {}", error.detail))?;
        authorizer
            .consume_human_approval(
                &space_id,
                &token,
                "entry.delete",
                Action::Delete,
                &ResourceRef {
                    kind: ResourceKind::Entry,
                    id: entry_id.clone(),
                    parent: None,
                },
                &replay_intent_hash,
                principal_id,
                actor_credential_id,
            )
            .await?;

        let mut identity = content_identity(principal_id, space_uid);
        identity.request_identity.credential_id = actor_credential_id;
        identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        identity.human_approval_token = Some(token);
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state);
        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/{entry_id}"))
                    .body(Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body: Value = serde_json::from_slice(&body)?;
        assert_eq!(body["code"], "HUMAN_APPROVAL_REPLAYED");
        Ok(())
    }

    #[tokio::test]
    async fn dangerous_route_returns_gone_for_an_expired_approval() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-human-approval-route-expired")?;
        let principal_id = Uuid::from_u128(1942);
        let space_id = state
            .service
            .create_space_for_principal("human-approval-route-expired", principal_id, "Route test")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        let actor_credential_id = Uuid::now_v7();
        let (entry_id, token) = issue_route_test_approval(
            &state,
            &space_id,
            principal_id,
            actor_credential_id,
            chrono::Duration::seconds(1),
        )
        .await?;
        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;

        let mut identity = content_identity(principal_id, space_uid);
        identity.request_identity.credential_id = actor_credential_id;
        identity.token_actions = Some(["delete".to_string()].into_iter().collect());
        identity.human_approval_token = Some(token);
        let route = Router::new()
            .route(
                "/spaces/{space_id}/entries/{entry_id}",
                delete(delete_entry),
            )
            .layer(Extension(identity))
            .with_state(state);
        let response = route
            .oneshot(
                Request::delete(format!("/spaces/{space_id}/entries/{entry_id}"))
                    .body(Body::empty())?,
            )
            .await?;
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        let body: Value = serde_json::from_slice(&body)?;
        assert_eq!(body["code"], "HUMAN_APPROVAL_EXPIRED");
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
                    credential_generation: 0,
                },
            ),
            created_by: owner_account_id,
        };
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid,
                principal_id,
                node_account_id: invited_account_id,
                binding_method: BindingMethod::Invite,
            })
            .await?;
        let authorizer = Authorizer::new(state.service.operator().clone());
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
        // The Node half is already durable, but the inviter's Node binding is
        // missing. This forces the failure boundary after the Node mutation;
        // adding that binding below must make the same request converge.
        assert!(
            bind_invited_account(&state, &account, &invitation, BindingMethod::Invite)
                .await
                .is_err()
        );
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid,
                principal_id: owner_principal_id,
                node_account_id: owner_account_id,
                binding_method: BindingMethod::Setup,
            })
            .await?;
        bind_invited_account(&state, &account, &invitation, BindingMethod::Invite)
            .await
            .expect("idempotent retry after missing inviter binding");

        authorizer
            .change_role(
                &space_uid.to_string(),
                owner_principal_id,
                owner_principal_id,
                SpaceRole::Viewer,
            )
            .await?;

        authorizer
            .revoke_principal(
                &space_uid.to_string(),
                backup_owner_principal_id,
                owner_principal_id,
            )
            .await?;

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

        // OIDC uses the same Node-first finalization contract. Exercise the
        // same post-Node failure boundary with its distinct binding method so
        // a callback retry cannot create a second principal.
        let oidc_owner_principal_id = Uuid::now_v7();
        let oidc_owner_account_id = Uuid::now_v7();
        let oidc_principal_id = Uuid::now_v7();
        let oidc_account_id = Uuid::now_v7();
        let oidc_space_uid = state
            .service
            .create_space_for_principal(
                "invitation-oidc-saga",
                oidc_owner_principal_id,
                "OIDC owner",
            )
            .await?;
        let oidc_account = HumanAccount {
            account_id: oidc_account_id,
            display_name: "OIDC viewer".to_string(),
            status: AccountStatus::Active,
            created_at: chrono::Utc::now().to_rfc3339(),
            node_roles: BTreeSet::new(),
            credential_generation: 0,
        };
        let oidc_invitation = AccountInvitation {
            invitation_id: Uuid::now_v7(),
            token_hash: "oidc-test".to_string(),
            display_name: oidc_account.display_name.clone(),
            space_uid: Some(oidc_space_uid),
            role: Some("viewer".to_string()),
            expires_at: chrono::Utc::now().to_rfc3339(),
            acceptance: Some(
                ugoite_identity::node_identity::InvitationAcceptance::Pending {
                    account_id: oidc_account_id,
                    principal_id: oidc_principal_id,
                    kind: ugoite_identity::node_identity::InvitationAcceptanceKind::Oidc,
                    claimed_at: chrono::Utc::now().to_rfc3339(),
                    credential_generation: 0,
                },
            ),
            created_by: oidc_owner_account_id,
        };
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid: oidc_space_uid,
                principal_id: oidc_principal_id,
                node_account_id: oidc_account_id,
                binding_method: BindingMethod::Oidc,
            })
            .await?;
        let oidc_authorizer = Authorizer::new(state.service.operator().clone());
        assert!(
            bind_invited_account(&state, &oidc_account, &oidc_invitation, BindingMethod::Oidc,)
                .await
                .is_err()
        );
        state
            .identity
            .add_binding(ugoite_domain::identity::PrincipalBinding {
                space_uid: oidc_space_uid,
                principal_id: oidc_owner_principal_id,
                node_account_id: oidc_owner_account_id,
                binding_method: BindingMethod::Setup,
            })
            .await?;
        bind_invited_account(&state, &oidc_account, &oidc_invitation, BindingMethod::Oidc)
            .await
            .expect("OIDC retry after missing inviter binding");
        assert!(oidc_authorizer
            .state(&oidc_space_uid.to_string())
            .await?
            .memberships
            .contains_key(&oidc_principal_id));
        Ok(())
    }

    #[tokio::test]
    async fn space_creation_retry_repairs_a_missing_node_binding() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-space-create-recovery")?;
        state.initialize_node().await?;
        let account_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("recover-space", principal_id, "Owner")
            .await?;

        let (recovered_uid, created) =
            ensure_local_space_owner_binding(&state, "recover-space", account_id, "Owner")
                .await
                .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        assert_eq!(recovered_uid, space_uid);
        assert!(!created);
        assert_eq!(
            state
                .identity
                .binding_for_account(space_uid, account_id)
                .await?,
            Some(principal_id)
        );

        let (same_uid, created) =
            ensure_local_space_owner_binding(&state, "recover-space", account_id, "Owner")
                .await
                .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        assert_eq!(same_uid, space_uid);
        assert!(!created);

        let partial_space_uid = state
            .service
            .create_space_for_principal("partial-recovery-space", Uuid::now_v7(), "Owner")
            .await?;
        let partial_settings_path = format!("spaces/{partial_space_uid}/settings.json");
        state
            .service
            .operator()
            .delete(&partial_settings_path)
            .await?;
        let error = ensure_local_space_owner_binding(
            &state,
            "partial-recovery-space",
            Uuid::now_v7(),
            "Owner",
        )
        .await
        .expect_err("incomplete Space bootstrap must not be finalized by recovery");
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        Ok(())
    }

    #[tokio::test]
    async fn non_local_space_creation_rejects_before_recovery_or_binding() -> anyhow::Result<()> {
        let state = AppState::new_for_tests("s3://ugoite-test-bucket/server-space")?;
        let error =
            ensure_local_space_owner_binding(&state, "remote-space", Uuid::now_v7(), "Owner")
                .await
                .expect_err("non-local Space creation must fail closed before recovery");
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.detail["code"], "STORAGE_MUTATION_UNAVAILABLE");

        let error = reconcile_recovery_fences_api(&state, "remote-space")
            .await
            .expect_err("recovery reconciliation must fail closed before remote writes");
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.detail["code"], "STORAGE_MUTATION_UNAVAILABLE");

        let error = reconcile_all_recovery_fences_api(&state)
            .await
            .expect_err("global recovery reconciliation must fail closed before remote writes");
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.detail["code"], "STORAGE_MUTATION_UNAVAILABLE");

        state
            .initialize_node()
            .await
            .expect("read-only remote startup must not require mutation capability");
        Ok(())
    }

    #[tokio::test]
    async fn space_creation_recovery_rejects_legacy_or_mismatched_authorization(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests("memory://server-space-create-recovery-validation")?;
        state.initialize_node().await?;
        let principal_id = Uuid::now_v7();
        let space_uid = state
            .service
            .create_space_for_principal("mismatched-space", principal_id, "Owner")
            .await?;
        let operator = state.service.operator().clone();
        let authorizer = Authorizer::new(operator.clone());
        let account_id = Uuid::now_v7();
        let mut authorization = authorizer.state(&space_uid.to_string()).await?;
        authorization.space_uid = Uuid::now_v7();
        let authorization_path = format!("spaces/{space_uid}/security/principals.json");
        operator
            .write(&authorization_path, serde_json::to_vec(&authorization)?)
            .await?;
        let state_error = authorizer
            .state(&space_uid.to_string())
            .await
            .expect_err("regular authorization reads must bind state to metadata");
        assert!(state_error
            .to_string()
            .contains("different space_uid values"));
        let validation_error = authorizer
            .validate_current_layout(&space_uid.to_string(), space_uid)
            .await
            .expect_err("mismatched authorization state must be rejected");
        assert!(validation_error
            .to_string()
            .contains("different space_uid values"));

        let error =
            ensure_local_space_owner_binding(&state, "mismatched-space", account_id, "Owner")
                .await
                .expect_err("mismatched authorization state must fail closed");
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state
            .identity
            .binding_for_account(space_uid, account_id)
            .await?
            .is_none());

        state
            .service
            .create_space_for_principal("legacy-space", Uuid::now_v7(), "Owner")
            .await?;
        let legacy_space_uid = state
            .service
            .space_id_by_slug("legacy-space")
            .await?
            .expect("legacy test Space exists");
        let legacy_marker_path = format!("spaces/{legacy_space_uid}/authorization.json");
        operator.write(&legacy_marker_path, b"{}".to_vec()).await?;
        let validation_error = authorizer
            .validate_current_layout(&legacy_space_uid, Uuid::now_v7())
            .await
            .expect_err("legacy authorization layout must be rejected");
        assert!(validation_error
            .to_string()
            .contains("unsupported Space layout"));
        let error =
            ensure_local_space_owner_binding(&state, "legacy-space", Uuid::now_v7(), "Owner")
                .await
                .expect_err("legacy authorization layout must fail closed");
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        Ok(())
    }

    fn knowledge_markdown(title: &str, body: &str) -> String {
        format!("---\nform: Entry\n---\n# {title}\n\n## Body\n{body}")
    }

    fn json_request(method: Method, uri: String, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .expect("JSON request")
    }

    async fn response_json(
        response: axum::response::Response,
    ) -> anyhow::Result<(StatusCode, Value)> {
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
        Ok((status, serde_json::from_slice(&body)?))
    }

    #[tokio::test]
    async fn issue_2037_public_apply_uses_opaque_version_tokens() -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-public-knowledge-apply-{}",
            Uuid::now_v7()
        ))?;
        let principal_id = Uuid::from_u128(20370);
        let space_id = state
            .service
            .create_space_for_principal("public-knowledge-apply", principal_id, "Route test")
            .await?
            .to_string();
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "name": "Entry",
                    "fields": {"Body": {"type": "markdown"}},
                    "allow_extra_attributes": "deny"
                }),
            )
            .await?;
        let space_uid = state.service.space_uid(&space_id).await?;
        let route = Router::new()
            .route("/spaces/{space_id}/apply", post(apply_operations))
            .layer(Extension(content_identity(principal_id, space_uid)))
            .with_state(state);

        let create_response = route
            .clone()
            .oneshot(json_request(
                Method::POST,
                format!("/spaces/{space_id}/apply"),
                json!({
                    "operations": [{
                        "kind": "create",
                        "id": "apply-entry",
                        "markdown": knowledge_markdown("Apply entry", "created")
                    }],
                    "run_id": "run-2037",
                    "message": "public apply contract"
                }),
            ))
            .await?;
        let (status, create_body) = response_json(create_response).await?;
        assert_eq!(status, StatusCode::OK, "{create_body}");
        assert_eq!(create_body["run_id"], "run-2037");
        let version_token = create_body["operations"][0]["revision_id"]
            .as_str()
            .expect("opaque version token")
            .to_owned();
        assert!(!version_token.is_empty());

        let update_response = route
            .oneshot(json_request(
                Method::POST,
                format!("/spaces/{space_id}/apply"),
                json!({
                    "operations": [{
                        "kind": "update",
                        "id": "apply-entry",
                        "version_token": version_token,
                        "markdown": knowledge_markdown("Apply entry", "updated")
                    }],
                    "run_id": "run-2037"
                }),
            ))
            .await?;
        let (status, update_body) = response_json(update_response).await?;
        assert_eq!(status, StatusCode::OK, "{update_body}");
        assert_eq!(update_body["operations"][0]["kind"], "update");
        assert!(update_body["operations"][0]["revision_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        Ok(())
    }

    #[tokio::test]
    async fn issue_2037_apply_remove_requires_the_exact_public_approval_intent(
    ) -> anyhow::Result<()> {
        let state = AppState::new_for_tests(format!(
            "memory://server-public-knowledge-approval-{}",
            Uuid::now_v7()
        ))?;
        state.initialize_node().await?;
        let issuer_account_id = Uuid::from_u128(20371);
        let issuer_principal_id = Uuid::from_u128(20372);
        let actor_account_id = Uuid::from_u128(20373);
        let actor_principal_id = Uuid::from_u128(20374);
        let issuer_credential_id = Uuid::from_u128(20375);
        let actor_credential_id = Uuid::from_u128(20376);
        let space_id = state
            .service
            .create_space_for_principal("public-knowledge-approval", issuer_principal_id, "Owner")
            .await?
            .to_string();
        let space_uid = state.service.space_uid(&space_id).await?;
        Authorizer::new(state.service.operator().clone())
            .add_human_member(
                &space_id,
                issuer_principal_id,
                SpacePrincipal {
                    principal_id: actor_principal_id,
                    kind: PrincipalKind::Human,
                    display_name: "Approval actor".into(),
                    state: PrincipalState::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                SpaceRole::Owner,
            )
            .await?;
        state
            .identity
            .seed_test_human_approval_credentials(
                space_uid,
                issuer_principal_id,
                issuer_account_id,
                actor_principal_id,
                actor_account_id,
                issuer_credential_id,
                actor_credential_id,
            )
            .await?;
        state
            .service
            .upsert_form(
                &space_id,
                &json!({
                    "name": "Entry",
                    "fields": {"Body": {"type": "markdown"}},
                    "allow_extra_attributes": "deny"
                }),
            )
            .await?;
        state
            .service
            .create_entry(
                &space_id,
                "approval-entry",
                &knowledge_markdown("Approval entry", "to remove"),
                &issuer_principal_id.to_string(),
            )
            .await?;

        let mut issuer_identity = content_identity(issuer_account_id, space_uid);
        issuer_identity.account_id = issuer_account_id;
        issuer_identity.request_identity.subject = AuthenticatedSubject::HumanAccount {
            account_id: issuer_account_id,
        };
        issuer_identity.request_identity.actor = Actor::Human {
            account_id: issuer_account_id,
        };
        issuer_identity.request_identity.credential_id = issuer_credential_id;
        issuer_identity.token_principal_id = None;
        issuer_identity.token_actor_principal_id = None;
        issuer_identity.token_space_uid = None;
        issuer_identity.token_actions = None;
        let issue_route = Router::new()
            .route("/spaces/{space_id}/approvals", post(issue_human_approval))
            .layer(Extension(issuer_identity))
            .with_state(state.clone());
        let approval_response = issue_route
            .oneshot(json_request(
                Method::POST,
                format!("/spaces/{space_id}/approvals"),
                json!({
                    "operation": "entry.delete",
                    "mutation": {"target_id": "approval-entry", "hard_delete": false},
                    "actor_credential_id": actor_credential_id,
                    "expires_in_seconds": 60
                }),
            ))
            .await?;
        let (status, approval_body) = response_json(approval_response).await?;
        assert_eq!(status, StatusCode::CREATED, "{approval_body}");
        assert_eq!(approval_body["operation"], "entry.delete");
        let approval_token = approval_body["approval_token"]
            .as_str()
            .expect("opaque approval token")
            .to_owned();
        assert_eq!(
            approval_body["intent_hash"].as_str().map(str::len),
            Some(64)
        );

        let mut actor_identity = content_identity(actor_account_id, space_uid);
        actor_identity.account_id = actor_account_id;
        actor_identity.request_identity.subject = AuthenticatedSubject::HumanAccount {
            account_id: actor_account_id,
        };
        actor_identity.request_identity.actor = Actor::Human {
            account_id: actor_account_id,
        };
        actor_identity.request_identity.credential_id = actor_credential_id;
        actor_identity.request_identity.authentication_method =
            RequestAuthenticationMethod::DeviceProof;
        actor_identity.request_identity.assurance = AssuranceLevel::Possession;
        actor_identity.token_principal_id = Some(actor_principal_id);
        actor_identity.token_actor_principal_id = Some(actor_principal_id);
        actor_identity.token_space_uid = Some(space_uid);
        actor_identity.token_actions =
            Some(["read", "delete"].into_iter().map(str::to_string).collect());
        actor_identity.human_approval_token = Some(approval_token);
        let apply_route = Router::new()
            .route("/spaces/{space_id}/apply", post(apply_operations))
            .layer(Extension(actor_identity))
            .with_state(state);

        let mismatch_response = apply_route
            .clone()
            .oneshot(json_request(
                Method::POST,
                format!("/spaces/{space_id}/apply"),
                json!({"operations": [{"kind": "remove", "id": "different-entry"}]}),
            ))
            .await?;
        let (status, mismatch_body) = response_json(mismatch_response).await?;
        assert_eq!(status, StatusCode::FORBIDDEN, "{mismatch_body}");
        assert_eq!(mismatch_body["code"], "HUMAN_APPROVAL_INVALID");
        assert!(mismatch_body["message"].as_str().is_some());

        let matching_response = apply_route
            .oneshot(json_request(
                Method::POST,
                format!("/spaces/{space_id}/apply"),
                json!({"operations": [{"kind": "remove", "id": "approval-entry"}]}),
            ))
            .await?;
        let (status, matching_body) = response_json(matching_response).await?;
        assert_eq!(status, StatusCode::OK, "{matching_body}");
        assert_eq!(
            matching_body["operations"][0],
            json!({
                "kind": "remove",
                "id": "approval-entry"
            })
        );
        Ok(())
    }
}
