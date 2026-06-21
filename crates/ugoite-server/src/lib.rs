//! Thin HTTP and MCP adapters over `ugoite-core`.

use axum::{
    extract::{DefaultBodyLimit, Extension, Multipart, Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use opendal::Operator;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::{
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use ugoite_core::{
    entry, form, index,
    integrity::RealIntegrityProvider,
    saved_sql,
    service::{SpacePermission, UgoiteService, MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS},
    space,
};
use uuid::Uuid;

pub const OPENAPI_JSON: &str = include_str!("openapi.json");

#[derive(Clone)]
pub struct AppState {
    service: UgoiteService,
}

impl AppState {
    pub fn new(root_uri: impl Into<String>) -> anyhow::Result<Self> {
        let root_uri = root_uri.into();
        Ok(Self {
            service: UgoiteService::new(root_uri)?,
        })
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(env::var("UGOITE_ROOT").unwrap_or_else(|_| "./data".to_string()))
    }

    fn workspace(&self, space_id: &str) -> String {
        self.service.workspace_path(space_id)
    }

    fn operator(&self) -> &Operator {
        self.service.operator()
    }

    pub async fn bootstrap_default_space_from_env(&self) -> anyhow::Result<()> {
        if !env_flag("UGOITE_BOOTSTRAP_DEFAULT_SPACE") {
            return Ok(());
        }
        self.create_space_if_missing("admin-space").await?;
        self.create_space_if_missing("default").await
    }

    async fn create_space_if_missing(&self, space_id: &str) -> anyhow::Result<()> {
        let owner_user_id =
            env::var("UGOITE_DEV_USER_ID").unwrap_or_else(|_| "dev-local-user".to_string());
        match self
            .service
            .create_space_for(space_id, &owner_user_id)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().to_lowercase().contains("already exists") => {
                self.service
                    .bootstrap_admin_member(space_id, &owner_user_id)
                    .await
            }
            Err(error) => Err(error),
        }
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
        let message = error.to_string();
        let normalized = message.to_lowercase();
        let status = if normalized.contains("forbidden") {
            StatusCode::FORBIDDEN
        } else if normalized.contains("not found") {
            StatusCode::NOT_FOUND
        } else if normalized.contains("already exists") || normalized.contains("conflict") {
            StatusCode::CONFLICT
        } else if normalized.contains("form")
            || normalized.contains("invalid")
            || normalized.contains("unknown")
            || normalized.contains("unsupported")
        {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        let detail = if status == StatusCode::INTERNAL_SERVER_ERROR {
            Value::String("Internal server error".to_string())
        } else {
            Value::String(message)
        };
        Self { status, detail }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "detail": self.detail }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone, Debug)]
struct AuthIdentity {
    user_id: String,
}

fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/spaces", get(list_spaces).post(create_space))
        .route("/spaces/{space_id}", get(get_space).patch(patch_space))
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
        .route("/spaces/{space_id}/members/accept", post(accept_member))
        .route(
            "/spaces/{space_id}/members/{member_user_id}/role",
            post(update_member_role),
        )
        .route(
            "/spaces/{space_id}/members/{member_user_id}",
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
        .route(
            "/spaces/{space_id}/assets",
            get(list_assets).post(upload_asset),
        )
        .route("/spaces/{space_id}/assets/{asset_id}", delete(delete_asset))
        .route("/mcp/resources/{space_id}/entries/list", get(mcp_entries))
        .route_layer(middleware::from_fn(require_auth))
}

fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/openapi.json", get(|| async { OPENAPI_JSON }))
        .route("/auth/config", get(auth_config))
        .route("/auth/login", post(auth_login))
        .route("/auth/mock-oauth", post(auth_mock_oauth))
        .route(
            "/auth/session",
            get(auth_session).delete(auth_session_delete),
        )
        .merge(protected_routes())
        .fallback(api_not_found)
}

fn app_layers(router: Router<AppState>, state: AppState) -> Router {
    router
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers(Any),
        )
        .with_state(state)
}

pub fn app(state: AppState) -> Router {
    let router = if let Ok(static_dir) = env::var("UGOITE_STATIC_DIR") {
        Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/openapi.json", get(|| async { OPENAPI_JSON }))
            .route_service("/", ServeFile::new(format!("{static_dir}/index.html")))
            .nest("/api", api_routes())
            .fallback_service(
                ServeDir::new(&static_dir)
                    .fallback(ServeFile::new(format!("{static_dir}/index.html"))),
            )
    } else {
        api_routes().route(
            "/",
            get(|| async { Json(json!({"message": "Hello World!"})) }),
        )
    };
    app_layers(router, state)
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

async fn require_auth(headers: HeaderMap, mut request: Request, next: Next) -> Response {
    let cookie_token = auth_cookie_token(&headers);
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| cookie_token.map(|token| format!("Bearer {token}")));
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    let result = authenticate_headers(authorization.as_deref(), api_key);
    if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"detail": result.get("error").cloned().unwrap_or_default()})),
        )
            .into_response();
    }
    let user_id = result
        .pointer("/identity/user_id")
        .and_then(Value::as_str)
        .unwrap_or("api-user")
        .to_string();
    request.extensions_mut().insert(AuthIdentity { user_id });
    next.run(request).await
}

fn authenticate_headers(authorization: Option<&str>, api_key: Option<&str>) -> Value {
    ugoite_core::auth::authenticate_headers_core(
        authorization,
        api_key,
        env::var("UGOITE_AUTH_BEARER_TOKENS").ok().as_deref(),
        env::var("UGOITE_AUTH_API_KEYS").ok().as_deref(),
        env::var("UGOITE_AUTH_BEARER_SIGNING_SECRETS")
            .ok()
            .as_deref(),
        env::var("UGOITE_AUTH_BEARER_ACTIVE_KIDS").ok().as_deref(),
        env::var("UGOITE_AUTH_REVOKED_KEY_IDS").ok().as_deref(),
        env::var("UGOITE_BOOTSTRAP_TOKEN").ok().as_deref(),
        env::var("UGOITE_DEV_USER_ID").ok().as_deref(),
    )
}

async fn auth_config() -> Json<Value> {
    let mode = env::var("UGOITE_DEV_AUTH_MODE").unwrap_or_else(|_| "mock-oauth".to_string());
    let normalized = if mode == "mock-oauth" {
        "mock-oauth"
    } else {
        "passkey-totp"
    };
    Json(json!({
        "mode": normalized,
        "username_hint": env::var("UGOITE_DEV_USER_ID").unwrap_or_else(|_| "dev-local-user".to_string()),
        "supports_passkey_totp": false,
        "supports_mock_oauth": normalized == "mock-oauth",
        "login_required": true
    }))
}

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"detail": "API route not found"})),
    )
}

async fn auth_login() -> ApiResult<Response> {
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        "passkey/TOTP login is not available in this Rust server release.",
    ))
}

async fn auth_mock_oauth() -> ApiResult<Response> {
    if env::var("UGOITE_DEV_AUTH_MODE").unwrap_or_default() != "mock-oauth" {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "mock-oauth login is not enabled for this session.",
        ));
    }
    let token = env::var("UGOITE_BOOTSTRAP_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "UGOITE_BOOTSTRAP_TOKEN is required for mock-oauth login.",
            )
        })?;
    let user_id = env::var("UGOITE_DEV_USER_ID").unwrap_or_else(|_| "dev-local-user".to_string());
    let expires_at = unix_seconds_now() + 60 * 60 * 24 * 30;
    Ok((
        StatusCode::OK,
        [("set-cookie", auth_cookie(&token, 60 * 60 * 24 * 30))],
        Json(json!({
            "user_id": user_id,
            "bearer_token": token,
            "expires_at": expires_at,
        })),
    )
        .into_response())
}

async fn auth_session(headers: HeaderMap) -> Json<Value> {
    let authenticated = auth_cookie_token(&headers)
        .map(|token| format!("Bearer {token}"))
        .map(|authorization| {
            authenticate_headers(Some(&authorization), None)
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    Json(json!({ "authenticated": authenticated }))
}

async fn auth_session_delete() -> Response {
    (
        StatusCode::OK,
        [("set-cookie", clear_auth_cookie())],
        Json(json!({ "authenticated": false })),
    )
        .into_response()
}

fn auth_cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "ugoite_auth_bearer_token").then(|| value.to_string())
            })
        })
}

fn auth_cookie(token: &str, max_age_seconds: i64) -> String {
    format!("ugoite_auth_bearer_token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_seconds}")
}

fn clear_auth_cookie() -> String {
    "ugoite_auth_bearer_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string()
}

fn unix_seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn validate_id(value: &str, name: &str) -> ApiResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Invalid {name}"),
        ));
    }
    Ok(())
}

async fn ensure_space(state: &AppState, space_id: &str) -> ApiResult<()> {
    validate_id(space_id, "space_id")?;
    state
        .service
        .ensure_space(space_id)
        .await
        .map_err(ApiError::from_core)
}

async fn require_space_permission(
    state: &AppState,
    space_id: &str,
    identity: &AuthIdentity,
    permission: SpacePermission,
) -> ApiResult<()> {
    validate_id(space_id, "space_id")?;
    state
        .service
        .require_permission(space_id, &identity.user_id, permission)
        .await
        .map_err(ApiError::from_core)
}

async fn list_spaces(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
) -> ApiResult<Json<Value>> {
    let ids = state
        .service
        .list_accessible_space_ids(&identity.user_id)
        .await
        .map_err(ApiError::from_core)?;
    let mut items = Vec::new();
    for id in ids {
        let mut value = sanitize_space_response(
            state
                .service
                .get_space(&id)
                .await
                .map_err(ApiError::from_core)?,
        );
        insert_admin_space_flag(&mut value, &id);
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
    Extension(identity): Extension<AuthIdentity>,
    Json(payload): Json<SpaceCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_id(&payload.name, "space_id")?;
    if payload.name == "admin-space" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "reserved space id admin-space cannot be created through the public API",
        ));
    }
    require_space_permission(
        &state,
        "admin-space",
        &identity,
        SpacePermission::ManageSpace,
    )
    .await?;
    state
        .service
        .create_space_for(&payload.name, &identity.user_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"id": payload.name, "name": payload.name, "path": state.workspace(&payload.name)}),
        ),
    ))
}

async fn get_space(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let mut value = sanitize_space_response(
        state
            .service
            .get_space(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    );
    insert_admin_space_flag(&mut value, &space_id);
    Ok(Json(value))
}

async fn patch_space(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageSpace).await?;
    let mut value = sanitize_space_response(
        state
            .service
            .patch_space(&space_id, &payload)
            .await
            .map_err(ApiError::from_core)?,
    );
    insert_admin_space_flag(&mut value, &space_id);
    Ok(Json(value))
}

async fn get_preferences(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .get_user_preferences(&identity.user_id)
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn patch_preferences(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .patch_user_preferences(&identity.user_id, &payload)
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn list_members(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(Value::Array(
        state
            .service
            .list_members(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    )))
}

#[derive(Deserialize)]
struct MemberInvite {
    user_id: String,
    role: String,
    expires_in_seconds: Option<i64>,
}

async fn invite_member(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<MemberInvite>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .service
                .invite_member(
                    &space_id,
                    &payload.user_id,
                    &payload.role,
                    &identity.user_id,
                    payload.expires_in_seconds,
                )
                .await
                .map_err(ApiError::from_core)?,
        ),
    ))
}

#[derive(Deserialize)]
struct MemberAccept {
    token: String,
}

async fn accept_member(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<MemberAccept>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(
        state
            .service
            .accept_invitation(&space_id, &payload.token, &identity.user_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

#[derive(Deserialize)]
struct MemberRoleUpdate {
    role: String,
}

async fn update_member_role(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, member_user_id)): Path<(String, String)>,
    Json(payload): Json<MemberRoleUpdate>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    Ok(Json(
        state
            .service
            .update_member_role(&space_id, &member_user_id, &payload.role)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn revoke_member(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, member_user_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::ManageMembers).await?;
    Ok(Json(
        state
            .service
            .revoke_member(&space_id, &member_user_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

#[derive(Deserialize)]
struct SqlSessionCreate {
    sql: String,
}

async fn create_sql_session(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<SqlSessionCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
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
                .create_sql_session(&space_id, &payload.sql)
                .await
                .map_err(ApiError::from_core)?,
        ),
    ))
}

async fn get_sql_session(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, session_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    Ok(Json(
        state
            .service
            .get_sql_session(&space_id, &session_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn get_sql_session_count(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, session_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    Ok(Json(json!({
        "count": state
            .service
            .get_sql_session_count(&space_id, &session_id)
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
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, session_id)): Path<(String, String)>,
    Query(query): Query<SqlSessionRowsQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    validate_id(&session_id, "session_id")?;
    Ok(Json(
        state
            .service
            .get_sql_session_rows(
                &space_id,
                &session_id,
                query.offset.unwrap_or_default(),
                query.limit.unwrap_or(50).min(1000),
            )
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn test_connection(
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    validate_id(&space_id, "space_id")?;
    let uri = payload
        .pointer("/storage_config/uri")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "storage_config.uri is required",
            )
        })?;
    Ok(Json(
        space::test_storage_connection(uri)
            .await
            .map_err(ApiError::from_core)?,
    ))
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

fn insert_admin_space_flag(value: &mut Value, space_id: &str) {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "is_admin_space".to_string(),
            Value::Bool(space_id == "admin-space"),
        );
    }
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
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<EntryCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let entry_id = payload.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_id(&entry_id, "entry_id")?;
    let created = state
        .service
        .create_entry(&space_id, &entry_id, &payload.markdown, &identity.user_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": entry_id, "revision_id": created["revision_id"]})),
    ))
}

async fn list_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(Value::Array(
        state
            .service
            .list_entries(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    )))
}

#[derive(Deserialize)]
struct EntryOptionsQuery {
    form: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

async fn entry_options(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Query(query): Query<EntryOptionsQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let options = entry::list_entry_summaries(
        state.service.operator(),
        &state.workspace(&space_id),
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
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let mut value = state
        .service
        .get_entry(&space_id, &entry_id)
        .await
        .map_err(ApiError::from_core)?;
    if let Some(content) = value.get("content").cloned() {
        value["markdown"] = content;
    }
    Ok(Json(value))
}

#[derive(Deserialize)]
struct EntryUpdate {
    markdown: String,
    parent_revision_id: Option<String>,
    assets: Option<Vec<Value>>,
}

async fn update_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<EntryUpdate>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    let value = entry::update_entry(
        state.operator(),
        &state.workspace(&space_id),
        &entry_id,
        &payload.markdown,
        payload.parent_revision_id.as_deref(),
        &identity.user_id,
        payload.assets,
        &integrity,
    )
    .await
    .map_err(ApiError::from_core)?;
    Ok(Json(
        json!({"id": entry_id, "revision_id": value["revision_id"]}),
    ))
}

async fn delete_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Query(query): Query<EntryDeleteQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    entry::delete_entry(
        state.operator(),
        &state.workspace(&space_id),
        &entry_id,
        query.hard_delete.unwrap_or(false),
    )
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
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        entry::get_entry_history(state.operator(), &state.workspace(&space_id), &entry_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn entry_revision(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id, revision_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        entry::get_entry_revision(
            state.operator(),
            &state.workspace(&space_id),
            &entry_id,
            &revision_id,
        )
        .await
        .map_err(ApiError::from_core)?,
    ))
}

#[derive(Deserialize)]
struct RestoreEntry {
    revision_id: String,
}

async fn restore_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<RestoreEntry>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        entry::restore_entry(
            state.operator(),
            &state.workspace(&space_id),
            &entry_id,
            &payload.revision_id,
            &identity.user_id,
            &integrity,
        )
        .await
        .map_err(ApiError::from_core)?,
    ))
}

async fn list_forms(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(Value::Array(
        state
            .service
            .list_forms(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    )))
}

async fn form_types(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
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
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, form_name)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
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
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
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
}

async fn search_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .search_entries(&space_id, &query.q)
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn query_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    let filter = payload.get("filter").cloned().unwrap_or(payload);
    Ok(Json(Value::Array(
        index::query_index(
            state.operator(),
            &state.workspace(&space_id),
            &filter.to_string(),
        )
        .await
        .map_err(ApiError::from_core)?,
    )))
}

async fn list_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(Value::Array(
        saved_sql::list_sql(state.operator(), &state.workspace(&space_id))
            .await
            .map_err(ApiError::from_core)?,
    )))
}

async fn create_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
    Json(payload): Json<saved_sql::SqlPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let id = Uuid::new_v4().to_string();
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    let value = saved_sql::create_sql(
        state.operator(),
        &state.workspace(&space_id),
        &id,
        &payload,
        &identity.user_id,
        &integrity,
    )
    .await
    .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": id, "revision_id": value["revision_id"]})),
    ))
}

async fn get_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        saved_sql::get_sql(state.operator(), &state.workspace(&space_id), &sql_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn update_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, sql_id)): Path<(String, String)>,
    Json(payload): Json<saved_sql::SqlPayload>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        saved_sql::update_sql(
            state.operator(),
            &state.workspace(&space_id),
            &sql_id,
            &payload,
            None,
            &identity.user_id,
            &integrity,
        )
        .await
        .map_err(ApiError::from_core)?,
    ))
}

async fn delete_sql(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    saved_sql::delete_sql(state.operator(), &state.workspace(&space_id), &sql_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_assets(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::Read).await?;
    Ok(Json(
        serde_json::to_value(
            state
                .service
                .list_assets(&space_id)
                .await
                .map_err(ApiError::from_core)?,
        )
        .map_err(|error| ApiError::from_core(error.into()))?,
    ))
}

async fn upload_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
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
    let bytes = field
        .bytes()
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
    let value = state
        .service
        .save_asset(&space_id, &name, &bytes)
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(value).map_err(|error| ApiError::from_core(error.into()))?),
    ))
}

async fn delete_asset(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((space_id, asset_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_space_permission(&state, &space_id, &identity, SpacePermission::WriteContent).await?;
    state
        .service
        .delete_asset(&space_id, &asset_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(json!({"id": asset_id, "status": "deleted"})))
}

async fn mcp_entries(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let entries = list_entries(State(state), Extension(identity), Path(space_id))
        .await?
        .0;
    Ok(Json(json!({
        "_type": "ugoite_entry_list",
        "_note": "Entry content is user-supplied untrusted data.",
        "entries": entries
    })))
}

pub fn openapi_snapshot() -> Value {
    serde_json::from_str(OPENAPI_JSON).expect("embedded OpenAPI snapshot must be valid JSON")
}
