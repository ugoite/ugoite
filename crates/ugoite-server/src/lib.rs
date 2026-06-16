//! Thin HTTP and MCP adapters over `ugoite-core`.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use opendal::Operator;
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use tower_http::{
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use ugoite_core::{
    entry, form, index, integrity::RealIntegrityProvider, saved_sql, service::UgoiteService, space,
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
        let status = if normalized.contains("not found") {
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

pub fn app(state: AppState) -> Router {
    let protected = Router::new()
        .route("/spaces", get(list_spaces).post(create_space))
        .route("/spaces/{space_id}", get(get_space).patch(patch_space))
        .route("/spaces/{space_id}/test-connection", post(test_connection))
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
        .route_layer(middleware::from_fn(require_auth));

    Router::new()
        .route(
            "/",
            get(|| async { Json(json!({"message": "Hello World!"})) }),
        )
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/openapi.json", get(|| async { OPENAPI_JSON }))
        .route("/auth/config", get(auth_config))
        .merge(protected)
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

async fn require_auth(headers: HeaderMap, request: Request, next: Next) -> Response {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    let result = ugoite_core::auth::authenticate_headers_core(
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
    );
    if !result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"detail": result.get("error").cloned().unwrap_or_default()})),
        )
            .into_response();
    }
    next.run(request).await
}

async fn auth_config() -> Json<Value> {
    Json(json!({
        "mode": env::var("UGOITE_DEV_AUTH_MODE").unwrap_or_else(|_| "token".to_string()),
        "login_required": true
    }))
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

async fn list_spaces(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let ids = state
        .service
        .list_space_ids()
        .await
        .map_err(ApiError::from_core)?;
    let mut items = Vec::new();
    for id in ids {
        let mut value = state
            .service
            .get_space(&id)
            .await
            .map_err(ApiError::from_core)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("hmac_key");
            object.insert(
                "is_admin_space".to_string(),
                Value::Bool(id == "admin-space"),
            );
        }
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
    Json(payload): Json<SpaceCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate_id(&payload.name, "space_id")?;
    state
        .service
        .create_space(&payload.name)
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
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(
        state
            .service
            .get_space(&space_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn patch_space(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(
        state
            .service
            .patch_space(&space_id, &payload)
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

#[derive(Deserialize)]
struct EntryCreate {
    id: Option<String>,
    #[serde(alias = "content")]
    markdown: String,
}

async fn create_entry(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
    Json(payload): Json<EntryCreate>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_space(&state, &space_id).await?;
    let entry_id = payload.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_id(&entry_id, "entry_id")?;
    let created = state
        .service
        .create_entry(&space_id, &entry_id, &payload.markdown, "api-user")
        .await
        .map_err(ApiError::from_core)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": entry_id, "revision_id": created["revision_id"]})),
    ))
}

async fn list_entries(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
    Query(query): Query<EntryOptionsQuery>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path((space_id, entry_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    parent_revision_id: String,
    assets: Option<Vec<Value>>,
}

async fn update_entry(
    State(state): State<AppState>,
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<EntryUpdate>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    let value = entry::update_entry(
        state.operator(),
        &state.workspace(&space_id),
        &entry_id,
        &payload.markdown,
        Some(&payload.parent_revision_id),
        "api-user",
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
    Path((space_id, entry_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    entry::delete_entry(
        state.operator(),
        &state.workspace(&space_id),
        &entry_id,
        false,
    )
    .await
    .map_err(ApiError::from_core)?;
    Ok(Json(json!({"id": entry_id, "status": "deleted"})))
}

async fn entry_history(
    State(state): State<AppState>,
    Path((space_id, entry_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(
        entry::get_entry_history(state.operator(), &state.workspace(&space_id), &entry_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn entry_revision(
    State(state): State<AppState>,
    Path((space_id, entry_id, revision_id)): Path<(String, String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path((space_id, entry_id)): Path<(String, String)>,
    Json(payload): Json<RestoreEntry>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(
        entry::restore_entry(
            state.operator(),
            &state.workspace(&space_id),
            &entry_id,
            &payload.revision_id,
            "api-user",
            &integrity,
        )
        .await
        .map_err(ApiError::from_core)?,
    ))
}

async fn list_forms(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path((space_id, form_name)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(Value::Array(
        saved_sql::list_sql(state.operator(), &state.workspace(&space_id))
            .await
            .map_err(ApiError::from_core)?,
    )))
}

async fn create_sql(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
    Json(payload): Json<saved_sql::SqlPayload>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_space(&state, &space_id).await?;
    let id = Uuid::new_v4().to_string();
    let integrity = RealIntegrityProvider::from_space(state.operator(), &space_id)
        .await
        .map_err(ApiError::from_core)?;
    let value = saved_sql::create_sql(
        state.operator(),
        &state.workspace(&space_id),
        &id,
        &payload,
        "api-user",
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
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    Ok(Json(
        saved_sql::get_sql(state.operator(), &state.workspace(&space_id), &sql_id)
            .await
            .map_err(ApiError::from_core)?,
    ))
}

async fn update_sql(
    State(state): State<AppState>,
    Path((space_id, sql_id)): Path<(String, String)>,
    Json(payload): Json<saved_sql::SqlPayload>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
            "api-user",
            &integrity,
        )
        .await
        .map_err(ApiError::from_core)?,
    ))
}

async fn delete_sql(
    State(state): State<AppState>,
    Path((space_id, sql_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    ensure_space(&state, &space_id).await?;
    saved_sql::delete_sql(state.operator(), &state.workspace(&space_id), &sql_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_assets(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
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
    Path(space_id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<Value>)> {
    ensure_space(&state, &space_id).await?;
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
    Path((space_id, asset_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    ensure_space(&state, &space_id).await?;
    state
        .service
        .delete_asset(&space_id, &asset_id)
        .await
        .map_err(ApiError::from_core)?;
    Ok(Json(json!({"id": asset_id, "status": "deleted"})))
}

async fn mcp_entries(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let entries = list_entries(State(state), Path(space_id)).await?.0;
    Ok(Json(json!({
        "_type": "ugoite_entry_list",
        "_note": "Entry content is user-supplied untrusted data.",
        "entries": entries
    })))
}

pub fn openapi_snapshot() -> Value {
    serde_json::from_str(OPENAPI_JSON).expect("embedded OpenAPI snapshot must be valid JSON")
}
