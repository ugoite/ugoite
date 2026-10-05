//! Closed DataFusion context for authorized Iceberg queries.
//!
//! The public type deliberately exposes only closed query operations. It never
//! returns a `SessionContext`, Catalog, provider, or SQL planner that could
//! resolve an unapproved object.

use anyhow::{anyhow, bail, Context, Result};
use datafusion::catalog::default_table_source::DefaultTableSource;
use datafusion::datasource::TableProvider;
use datafusion::execution::context::SessionContext;
use datafusion::execution::memory_pool::GreedyMemoryPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::execution::{SessionStateBuilder, SessionStateDefaults};
use datafusion::logical_expr::expr_fn::ident;
use datafusion::logical_expr::{Expr, LogicalPlan, ScalarUDF, SortExpr};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::{col, lit, DataFrame, SessionConfig};
use iceberg_datafusion::IcebergStaticTableProvider;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use ugoite_core::query::{AuthorizedQueryPolicy, EntryScope, QuerySystemColumn};
use ugoite_core::sql_query::{
    SqlResultColumn, SqlResultColumnType, MAX_SQL_COLUMN_METADATA_BYTES, MAX_SQL_COLUMN_NAME_BYTES,
    MAX_SQL_OUTPUT_COLUMNS,
};
use ugoite_domain::form::sql_column_name;

use crate::{form_from_table, IcebergWorkspace};

const INTERNAL_RELATION_PREFIX: &str = "__ugoite_authorized_source_";

/// Number of DataFusion execution streams currently being consumed by this
/// process. The count covers stream polling, not query planning or response
/// serialization, and returns to its prior value when execution is cancelled.
static ACTIVE_QUERY_STREAMS: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of DataFusion streams currently being consumed.
pub fn active_query_streams() -> usize {
    ACTIVE_QUERY_STREAMS.load(Ordering::Relaxed)
}

struct ActiveQueryStream {
    query_active: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl ActiveQueryStream {
    fn new(query_active: Option<Arc<std::sync::atomic::AtomicBool>>) -> Self {
        ACTIVE_QUERY_STREAMS.fetch_add(1, Ordering::Relaxed);
        if let Some(query_active) = &query_active {
            query_active.store(true, Ordering::SeqCst);
        }
        Self { query_active }
    }
}

impl Drop for ActiveQueryStream {
    fn drop(&mut self) {
        ACTIVE_QUERY_STREAMS.fetch_sub(1, Ordering::Relaxed);
        if let Some(query_active) = &self.query_active {
            query_active.store(false, Ordering::SeqCst);
        }
    }
}

pub(crate) fn preserved_unnest_column(input: &str) -> String {
    format!("__ugoite_preserved_{input}")
}

/// Canonical lazy current-state derivation for append-only Form revision
/// tables. Every reader starts from this plan: entry authorization is applied
/// before the maximum-version aggregate, and tombstones are removed only after
/// that aggregate selected the latest revision. Keeping this as a DataFusion
/// builder preserves optimizer visibility and avoids a second implementation
/// in the SQL path.
pub(crate) fn latest_revision_dataframe(
    revisions: DataFrame,
    entry_scope: &EntryScope,
    view: crate::RevisionView,
) -> Result<DataFrame> {
    latest_revision_dataframe_after(revisions, entry_scope, view, None)
}

/// Applies the caller's authorization scope before exposing a revision
/// relation to any lookup plan.
pub(crate) fn apply_entry_scope(
    revisions: DataFrame,
    entry_scope: &EntryScope,
) -> Result<DataFrame> {
    match entry_scope {
        EntryScope::AllCurrent => Ok(revisions),
        EntryScope::Only(entry_ids) if entry_ids.is_empty() => Ok(revisions.filter(lit(false))?),
        EntryScope::Only(entry_ids) => Ok(revisions.filter(
            col("entry_id").in_list(
                entry_ids
                    .iter()
                    .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
                false,
            ),
        )?),
        EntryScope::AllExcept(entry_ids) if entry_ids.is_empty() => Ok(revisions),
        EntryScope::AllExcept(entry_ids) => Ok(revisions.filter(
            col("entry_id").in_list(
                entry_ids
                    .iter()
                    .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
                true,
            ),
        )?),
    }
}

/// Builds one ordered keyset page of the latest revision view. The cursor
/// predicate is applied before the max-version aggregate so a maintenance
/// rebuild never materializes the entire current Entry set merely to return a
/// bounded page.
pub(crate) fn latest_revision_dataframe_after(
    revisions: DataFrame,
    entry_scope: &EntryScope,
    view: crate::RevisionView,
    after_entry_id: Option<&[u8]>,
) -> Result<DataFrame> {
    let scoped = apply_entry_scope(revisions, entry_scope)?;
    let scoped = if let Some(after_entry_id) = after_entry_id {
        scoped.filter(col("entry_id").gt(lit(after_entry_id.to_vec())))?
    } else {
        scoped
    };
    let maxima = scoped
        .clone()
        .aggregate(
            vec![col("entry_id")],
            vec![
                datafusion::functions_aggregate::expr_fn::max(col("entry_version"))
                    .alias("latest_entry_version"),
            ],
        )?
        .select(vec![
            col("entry_id").alias("latest_entry_id"),
            col("latest_entry_version"),
        ])?;
    let heads = scoped.join(
        maxima,
        datafusion::logical_expr::JoinType::Inner,
        &["entry_id", "entry_version"],
        &["latest_entry_id", "latest_entry_version"],
        None,
    )?;
    // Never silently discard a duplicate maximum version. Consumers validate
    // the resulting cardinality as an append-only-history invariant and fail
    // the query rather than selecting an arbitrary revision.
    if view == crate::RevisionView::Current {
        Ok(heads.filter(col("operation").not_eq(lit("delete")))?)
    } else {
        Ok(heads)
    }
}

/// A query surface containing only Core-authorized, read-only logical Form
/// views. The underlying context remains private so callers cannot register a
/// table, UDF, object store, or provider of their own.
pub struct AuthorizedQueryContext {
    context: SessionContext,
    limits: ugoite_core::query::QueryLimits,
    permits: Arc<Semaphore>,
    authorized_relations: BTreeSet<String>,
    form_name_aliases: BTreeMap<String, String>,
    authorized_scans: BTreeSet<AuthorizedScan>,
    duplicate_head_checks: Vec<(Arc<dyn TableProvider>, DataFrame)>,
    duplicate_head_checks_validated: Arc<AsyncMutex<BTreeSet<usize>>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AuthorizedScan {
    table_uuid: String,
    snapshot_id: Option<i64>,
}

/// Closed public errors for the authorization-aware query surface. Upstream
/// DataFusion, Iceberg, and OpenDAL details remain available through the error
/// source chain for internal diagnostics, but are never included in `Display`.
#[derive(Debug)]
pub enum AuthorizedQueryError {
    InvalidQuery { source: anyhow::Error },
    UnauthorizedQueryFeature { source: anyhow::Error },
    ResourceLimitExceeded { source: anyhow::Error },
    RevisionInvariantViolation,
    QueryTimedOut,
    QueryExecutionFailed { source: anyhow::Error },
}

impl AuthorizedQueryError {
    fn invalid_query(source: impl Into<anyhow::Error>) -> Self {
        Self::InvalidQuery {
            source: source.into(),
        }
    }

    fn unauthorized(source: impl Into<anyhow::Error>) -> Self {
        Self::UnauthorizedQueryFeature {
            source: source.into(),
        }
    }

    fn resource_limit(source: impl Into<anyhow::Error>) -> Self {
        Self::ResourceLimitExceeded {
            source: source.into(),
        }
    }

    fn execution_failed(source: impl Into<anyhow::Error>) -> Self {
        Self::QueryExecutionFailed {
            source: source.into(),
        }
    }
}

pub(crate) fn bounded_session_context(
    limits: &ugoite_core::query::QueryLimits,
) -> Result<SessionContext> {
    limits.validate().map_err(|message| anyhow!(message))?;
    let config = SessionConfig::new()
        .with_information_schema(false)
        .with_target_partitions(limits.max_concurrency);
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_pool(Arc::new(GreedyMemoryPool::new(limits.max_memory_bytes)))
        .build_arc()
        .context("configure bounded DataFusion runtime")?;
    let allowed_functions = &limits.allowed_functions;
    let state = SessionStateBuilder::new()
        .with_config(config)
        .with_runtime_env(runtime)
        .with_expr_planners(SessionStateDefaults::default_expr_planners())
        .with_scalar_functions(allowed_scalar_functions(allowed_functions))
        .with_aggregate_functions(
            SessionStateDefaults::default_aggregate_functions()
                .into_iter()
                .filter(|function| {
                    allowed_functions.contains(&function.name().to_ascii_lowercase())
                })
                .collect(),
        )
        .with_window_functions(
            SessionStateDefaults::default_window_functions()
                .into_iter()
                .filter(|function| {
                    allowed_functions.contains(&function.name().to_ascii_lowercase())
                })
                .collect(),
        )
        .with_table_function_list(Vec::new())
        .build();
    Ok(SessionContext::new_with_state(state))
}

fn allowed_scalar_functions(allowed_functions: &BTreeSet<String>) -> Vec<Arc<ScalarUDF>> {
    let mut functions = SessionStateDefaults::default_scalar_functions()
        .into_iter()
        .filter(|function| allowed_functions.contains(&function.name().to_ascii_lowercase()))
        .collect::<Vec<_>>();
    if allowed_functions.contains(crate::search_normalization::SEARCH_NORMALIZE_FUNCTION_NAME) {
        functions.push(crate::search_normalization::search_normalize_udf());
    }
    functions
}

async fn collect_query_stream(
    stream: datafusion::physical_plan::SendableRecordBatchStream,
) -> Result<Vec<arrow_array::RecordBatch>> {
    collect_query_stream_with_probe(stream, None).await
}

async fn collect_query_stream_with_probe(
    stream: datafusion::physical_plan::SendableRecordBatchStream,
    query_active: Option<Arc<std::sync::atomic::AtomicBool>>,
) -> Result<Vec<arrow_array::RecordBatch>> {
    // Keep the stream in this future's scope. Dropping an HTTP request future
    // drops this collector, which drops the DataFusion stream and aborts its
    // execution tasks. The guard makes active execution independently
    // observable during that lifetime and is released on every exit path.
    let _active_stream = ActiveQueryStream::new(query_active);
    let mut stream = stream;
    let mut batches = Vec::new();
    while let Some(batch) = futures::TryStreamExt::try_next(&mut stream)
        .await
        .map_err(classify_datafusion_error)?
    {
        batches.push(batch);
    }
    Ok(batches)
}

#[cfg(feature = "test-support")]
pub async fn run_slow_query_for_test(
    dropped: Arc<std::sync::atomic::AtomicBool>,
    query_active: Arc<std::sync::atomic::AtomicBool>,
    started: Arc<tokio::sync::Notify>,
) {
    struct SlowSource {
        dropped: Arc<std::sync::atomic::AtomicBool>,
        started: Arc<tokio::sync::Notify>,
        first_poll: bool,
    }

    impl futures::Stream for SlowSource {
        type Item =
            std::result::Result<arrow_array::RecordBatch, datafusion::error::DataFusionError>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if self.first_poll {
                self.first_poll = false;
                self.started.notify_one();
            }
            std::task::Poll::Pending
        }
    }

    impl Drop for SlowSource {
        fn drop(&mut self) {
            self.dropped
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let stream = Box::pin(
        datafusion::physical_plan::stream::RecordBatchStreamAdapter::new(
            Arc::new(arrow_schema::Schema::empty()),
            SlowSource {
                dropped,
                started,
                first_poll: true,
            },
        ),
    );
    let _ = collect_query_stream_with_probe(stream, Some(query_active)).await;
}

fn classify_datafusion_error(error: datafusion::error::DataFusionError) -> AuthorizedQueryError {
    if matches!(
        error.find_root(),
        datafusion::error::DataFusionError::ResourcesExhausted(_)
    ) {
        AuthorizedQueryError::resource_limit(error)
    } else {
        AuthorizedQueryError::execution_failed(error)
    }
}

impl std::fmt::Display for AuthorizedQueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidQuery { .. } => "invalid authorized query",
            Self::UnauthorizedQueryFeature { .. } => "unauthorized query feature",
            Self::ResourceLimitExceeded { .. } => "authorized query resource limit exceeded",
            Self::RevisionInvariantViolation => {
                "entry revision invariant failed: multiple revisions share a maximum entry_version"
            }
            Self::QueryTimedOut => "authorized query timed out",
            Self::QueryExecutionFailed { .. } => "authorized query execution failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AuthorizedQueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidQuery { source }
            | Self::UnauthorizedQueryFeature { source }
            | Self::ResourceLimitExceeded { source }
            | Self::QueryExecutionFailed { source } => Some(source.as_ref()),
            Self::RevisionInvariantViolation => None,
            Self::QueryTimedOut => None,
        }
    }
}

impl IcebergWorkspace {
    /// Translates a Core authorization decision into a closed DataFusion query
    /// surface. All providers are static Iceberg providers; a requested
    /// checkpoint requires one snapshot for every exposed Form.
    pub async fn authorized_query_context(
        &self,
        policy: AuthorizedQueryPolicy,
    ) -> Result<AuthorizedQueryContext> {
        Ok(self
            .authorized_query_context_inner(policy)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?)
    }

    async fn authorized_query_context_inner(
        &self,
        policy: AuthorizedQueryPolicy,
    ) -> Result<AuthorizedQueryContext> {
        // Start from an empty SessionState. Registering only Core-approved
        // built-ins makes every other scalar, aggregate, window, and table
        // function unresolvable before plan validation. The empty default
        // catalog is retained solely for relation registration; no file
        // formats, table factories, function factory, or table functions are
        // installed.
        let context = bounded_session_context(&policy.limits)?;
        let mut relations = BTreeSet::new();
        let mut form_name_aliases = BTreeMap::new();
        let mut authorized_scans = BTreeSet::new();
        let mut duplicate_head_checks = Vec::new();

        let checkpoint = if let Some(checkpoint) = &policy.checkpoint {
            self.validate_checkpoint(checkpoint)?;
            let catalog = self
                .space_catalog
                .as_ref()
                .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?;
            let publication = catalog.publication_ref_for_checkpoint(checkpoint)?;
            let resolved = catalog.resolve_publication_checkpoint(&publication).await?;
            if resolved.coordinate_checksum != checkpoint.coordinate_checksum {
                return Err(anyhow!(
                    "authorized query coordinate is not reachable from the Catalog Head"
                ));
            }
            Some(resolved)
        } else {
            None
        };

        for (form_id, form_policy) in &policy.forms {
            validate_relation(&form_policy.relation)?;
            if !relations.insert(form_policy.relation.clone()) {
                bail!(
                    "authorized query policy repeats relation {}",
                    form_policy.relation
                );
            }
            for alias in &form_policy.sql_aliases {
                validate_form_name(alias)?;
                if is_legacy_relation_name(alias)
                    || alias.starts_with(INTERNAL_RELATION_PREFIX)
                    || relations.contains(alias)
                {
                    bail!("Form name {alias} collides with a reserved SQL relation");
                }
                if form_name_aliases
                    .insert(alias.clone(), form_policy.relation.clone())
                    .is_some()
                {
                    bail!("authorized query policy repeats Form name {alias}");
                }
            }
            let (form, table, expected_snapshot_id) = match &checkpoint {
                Some(checkpoint) => {
                    let coordinate = checkpoint
                        .tables
                        .iter()
                        .find(|coordinate| coordinate.form_id == *form_id)
                        .ok_or_else(|| {
                            anyhow!("checkpoint is missing authorized Form {form_id}")
                        })?;
                    let table = self
                        .space_catalog
                        .as_ref()
                        .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
                        .load_checkpoint_table(checkpoint, coordinate)
                        .await?;
                    (
                        form_from_table(&table, *form_id)?,
                        table,
                        coordinate.snapshot_id,
                    )
                }
                None => (
                    self.load_form(*form_id).await?,
                    self.catalog.load_table(&self.form_ident(*form_id)).await?,
                    None,
                ),
            };
            let snapshot_id = table.metadata().current_snapshot_id();
            if expected_snapshot_id.is_some_and(|expected| Some(expected) != snapshot_id) {
                bail!("Iceberg table snapshot does not match the authorized coordinate");
            }
            let current_snapshot_id = table.metadata().current_snapshot_id();
            let authorized_scan = AuthorizedScan {
                table_uuid: table.metadata().uuid().to_string(),
                snapshot_id: current_snapshot_id,
            };
            let provider: Arc<dyn TableProvider> = match current_snapshot_id {
                Some(snapshot_id) => Arc::new(
                    crate::read_schema_provider::CurrentSchemaTableProvider::try_new(
                        table,
                        snapshot_id,
                    )
                    .await
                    .context("open current-schema Iceberg provider")?,
                ),
                None => Arc::new(
                    IcebergStaticTableProvider::try_new_from_table(table)
                        .await
                        .context("open static Iceberg provider")?,
                ),
            };

            authorized_scans.insert(authorized_scan);

            let internal = format!("{INTERNAL_RELATION_PREFIX}{}", form_id.as_uuid().simple());
            relations.insert(internal.clone());
            context.register_table(internal.as_str(), provider.clone())?;
            let visible = visible_columns(&form, form_policy, provider.schema().as_ref())?;
            // Project before deriving latest revisions. This keeps opaque
            // Form columns out of the physical scan when a closed query
            // surface intentionally exposes only a safe subset, such as
            // keyword search. The revision derivation still receives the
            // identity columns it needs for authorization and head checks.
            let mut source_names = BTreeSet::new();
            let mut source_columns = Vec::new();
            for column in &visible {
                if source_names.insert(column.source.clone()) {
                    source_columns.push(ident(&column.source));
                }
            }
            for source in ["entry_id", "entry_version", "operation"] {
                if source_names.insert(source.to_string()) {
                    source_columns.push(ident(source));
                }
            }
            let source = context
                .table(internal.as_str())
                .await?
                .select(source_columns)
                .context("project authorized query source")?;
            let heads = latest_revision_dataframe(
                source,
                &form_policy.entry_scope,
                crate::RevisionView::LatestIncludingTombstones,
            )?
            .clone();
            // Key this plan by the provider identity rather than a relation
            // name. DataFusion can expand views and rewrite aliases before
            // this boundary, but the TableScan retains the approved provider.
            let duplicate_head_check = heads
                .clone()
                .aggregate(
                    vec![col("entry_id")],
                    vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                        .alias("ugoite_latest_head_count")],
                )?
                .filter(col("ugoite_latest_head_count").gt(lit(1)))?
                .limit(0, Some(1))?;
            duplicate_head_checks.push((provider, duplicate_head_check));
            let view = heads
                .filter(col("operation").not_eq(lit("delete")))?
                .select(
                    visible
                        .iter()
                        .map(|column| ident(&column.source).alias(&column.name))
                        .collect::<Vec<_>>(),
                )?
                .into_view();
            context.deregister_table(internal.as_str())?;
            context.register_table(form_policy.relation.as_str(), view.clone())?;
        }

        let permits = self.shared_query_permits(policy.limits.max_concurrency);
        Ok(AuthorizedQueryContext {
            context,
            limits: policy.limits,
            permits,
            authorized_relations: relations,
            form_name_aliases,
            authorized_scans,
            duplicate_head_checks,
            duplicate_head_checks_validated: Arc::new(AsyncMutex::new(BTreeSet::new())),
        })
    }
}

impl IcebergWorkspace {
    /// Builds the same closed latest-revision context used by ordinary reads,
    /// while retaining the workspace-wide permit pool. This is used for
    /// storage-level revision projections that cannot be exposed as a normal
    /// Form relation.
    pub(crate) async fn authorized_revision_query_context(
        &self,
        provider: Arc<dyn TableProvider>,
        table_uuid: String,
        snapshot_id: Option<i64>,
        entry_scope: &EntryScope,
        limits: ugoite_core::query::QueryLimits,
    ) -> Result<AuthorizedQueryContext> {
        let permits = self.shared_query_permits(limits.max_concurrency);
        self.authorized_revision_query_context_with_permits(
            provider,
            table_uuid,
            snapshot_id,
            entry_scope,
            limits,
            permits,
        )
        .await
    }

    /// Registers a checkpoint-pinned revision source with its Entry scope
    /// applied before any caller-specific lookup predicates.
    pub(crate) async fn authorized_revision_lookup_context(
        &self,
        provider: Arc<dyn TableProvider>,
        table_uuid: String,
        snapshot_id: Option<i64>,
        entry_scope: &EntryScope,
        limits: ugoite_core::query::QueryLimits,
    ) -> Result<AuthorizedQueryContext> {
        let context = bounded_session_context(&limits)?;
        context.register_table("revisions", provider.clone())?;
        let scoped_revisions =
            apply_entry_scope(context.table("revisions").await?, entry_scope)?.into_view();
        context.deregister_table("revisions")?;
        context.register_table("revisions", scoped_revisions)?;
        let permits = self.shared_query_permits(limits.max_concurrency);
        Ok(AuthorizedQueryContext {
            context,
            limits,
            permits,
            authorized_relations: BTreeSet::from(["revisions".to_string()]),
            form_name_aliases: BTreeMap::new(),
            authorized_scans: BTreeSet::from([AuthorizedScan {
                table_uuid,
                snapshot_id,
            }]),
            duplicate_head_checks: Vec::new(),
            duplicate_head_checks_validated: Arc::new(AsyncMutex::new(BTreeSet::new())),
        })
    }

    pub(crate) async fn authorized_revision_query_context_with_permits(
        &self,
        provider: Arc<dyn TableProvider>,
        table_uuid: String,
        snapshot_id: Option<i64>,
        entry_scope: &EntryScope,
        limits: ugoite_core::query::QueryLimits,
        permits: Arc<Semaphore>,
    ) -> Result<AuthorizedQueryContext> {
        let context = bounded_session_context(&limits)?;
        context.register_table("revisions", provider.clone())?;
        let source = context.table("revisions").await?;
        let heads = latest_revision_dataframe(
            source,
            entry_scope,
            crate::RevisionView::LatestIncludingTombstones,
        )?;
        let duplicate_head_check = heads
            .clone()
            .aggregate(
                vec![col("entry_id")],
                vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                    .alias("ugoite_latest_head_count")],
            )?
            .filter(col("ugoite_latest_head_count").gt(lit(1)))?
            .limit(0, Some(1))?;
        Ok(AuthorizedQueryContext {
            context,
            limits: limits.clone(),
            permits,
            authorized_relations: BTreeSet::from(["revisions".to_string()]),
            form_name_aliases: BTreeMap::new(),
            authorized_scans: BTreeSet::from([AuthorizedScan {
                table_uuid,
                snapshot_id,
            }]),
            duplicate_head_checks: vec![(provider, duplicate_head_check)],
            duplicate_head_checks_validated: Arc::new(AsyncMutex::new(BTreeSet::new())),
        })
    }
}

impl AuthorizedQueryContext {
    pub(crate) async fn relation_columns(&self, relation: &str) -> Result<Vec<String>> {
        let frame = self
            .context
            .table(relation)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        Ok(frame
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .collect())
    }

    /// Executes the bounded latest-head projection through this context's
    /// shared permit, timeout, provider validation, row bound, and invariant
    /// checks. Full history remains a separate audit operation.
    pub(crate) async fn execute_latest_revision_plan(
        &self,
        entry_scope: &EntryScope,
        view: crate::RevisionView,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let source = self
            .context
            .table("revisions")
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let heads = latest_revision_dataframe(
            source,
            entry_scope,
            crate::RevisionView::LatestIncludingTombstones,
        )
        .map_err(AuthorizedQueryError::invalid_query)?;
        let selected = match view {
            crate::RevisionView::Current => heads
                .filter(col("operation").not_eq(lit("delete")))
                .map_err(AuthorizedQueryError::invalid_query)?,
            crate::RevisionView::LatestIncludingTombstones => heads,
            crate::RevisionView::All => {
                return Err(AuthorizedQueryError::invalid_query(anyhow!(
                    "bounded latest revision plan does not support full history"
                ))
                .into())
            }
        };
        let limit = self
            .limits
            .max_rows
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorized query row limit is too large"))?;
        let frame = selected
            .select_columns(&["entry_id", "revision_id", "entry_version"])
            .map_err(AuthorizedQueryError::invalid_query)?
            .limit(0, Some(limit))
            .map_err(AuthorizedQueryError::invalid_query)?;
        self.execute_frame(frame, limit).await
    }

    pub(crate) async fn execute_latest_revision_page(
        &self,
        entry_scope: &EntryScope,
        view: crate::RevisionView,
        after_entry_id: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        if limit == 0 || limit > self.limits.max_rows {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "latest revision page exceeds its configured row limit"
            ))
            .into());
        }
        let source = self
            .context
            .table("revisions")
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let heads = latest_revision_dataframe_after(
            source,
            entry_scope,
            crate::RevisionView::LatestIncludingTombstones,
            after_entry_id,
        )
        .map_err(AuthorizedQueryError::invalid_query)?;
        let selected = match view {
            crate::RevisionView::Current => heads
                .filter(col("operation").not_eq(lit("delete")))
                .map_err(AuthorizedQueryError::invalid_query)?,
            crate::RevisionView::LatestIncludingTombstones => heads,
            crate::RevisionView::All => {
                return Err(AuthorizedQueryError::invalid_query(anyhow!(
                    "bounded latest revision page does not support full history"
                ))
                .into())
            }
        };
        let frame = selected
            .sort(vec![SortExpr {
                expr: col("entry_id"),
                asc: true,
                nulls_first: true,
            }])
            .map_err(AuthorizedQueryError::invalid_query)?
            .limit(0, Some(limit))
            .map_err(AuthorizedQueryError::invalid_query)?;
        self.execute_frame(frame, limit).await
    }

    /// Executes a trusted relation plan assembled by a typed read surface.
    /// The caller can request unnesting for a typed list, but cannot provide a
    /// provider, relation, catalog, or arbitrary SQL object. The same permit,
    /// timeout, physical-provider validation, row bound, and latest-head
    /// invariant checks as SQL execution are applied before Arrow leaves this
    /// context.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_relation_plan(
        &self,
        relation: &str,
        unnest_columns: &[(String, String)],
        predicates: Vec<Expr>,
        projection: Vec<Expr>,
        sort: Vec<SortExpr>,
        distinct: bool,
        preserve_unnest_columns: bool,
        limit: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_relation_plan_with_byte_limit(
            relation,
            unnest_columns,
            predicates,
            projection,
            sort,
            distinct,
            preserve_unnest_columns,
            limit,
            None,
        )
        .await
    }

    /// Executes a trusted relation plan while rejecting oversized Arrow
    /// materialization batches before callers convert them to owned JSON.
    /// AssetText authorization uses this narrower path because a row-count
    /// limit alone does not bound a large value or AssetReference payload.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_relation_plan_bounded(
        &self,
        relation: &str,
        unnest_columns: &[(String, String)],
        predicates: Vec<Expr>,
        projection: Vec<Expr>,
        sort: Vec<SortExpr>,
        distinct: bool,
        preserve_unnest_columns: bool,
        limit: usize,
        max_bytes: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_relation_plan_with_byte_limit(
            relation,
            unnest_columns,
            predicates,
            projection,
            sort,
            distinct,
            preserve_unnest_columns,
            limit,
            Some(max_bytes),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_relation_plan_with_byte_limit(
        &self,
        relation: &str,
        unnest_columns: &[(String, String)],
        predicates: Vec<Expr>,
        projection: Vec<Expr>,
        sort: Vec<SortExpr>,
        distinct: bool,
        preserve_unnest_columns: bool,
        limit: usize,
        max_bytes: Option<usize>,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        if limit == 0 || limit > self.limits.max_rows.saturating_add(1) {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "authorized relation plan exceeds its configured row limit"
            ))
            .into());
        }
        let mut frame = self
            .context
            .table(relation)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        if preserve_unnest_columns && !unnest_columns.is_empty() {
            let mut preserved_projection = frame
                .schema()
                .fields()
                .iter()
                .map(|field| col(field.name()))
                .collect::<Vec<_>>();
            preserved_projection.extend(unnest_columns.iter().map(|(input_column, _)| {
                col(input_column).alias(preserved_unnest_column(input_column))
            }));
            frame = frame
                .select(preserved_projection)
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        for (input_column, output_column) in unnest_columns {
            frame = frame
                .unnest_columns_with_options(
                    &[input_column.as_str()],
                    datafusion::common::UnnestOptions::new().with_recursions(
                        datafusion::common::RecursionUnnestOption {
                            input_column: input_column.clone().into(),
                            output_column: output_column.clone().into(),
                            depth: 1,
                        },
                    ),
                )
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        for predicate in predicates {
            frame = frame
                .filter(predicate)
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        if !projection.is_empty() {
            frame = frame
                .select(projection)
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        if distinct {
            frame = frame
                .distinct()
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        if !sort.is_empty() {
            frame = frame
                .sort(sort)
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        let frame = frame
            .limit(0, Some(limit))
            .map_err(AuthorizedQueryError::invalid_query)?;
        self.execute_frame_with_byte_limit(frame, limit, max_bytes)
            .await
    }

    /// Runs a DataFusion aggregate over one authorized relation. Statistics
    /// use this path so row counts and tag counts never require a Rust-side
    /// current-state scan.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_relation_aggregate_plan(
        &self,
        relation: &str,
        unnest_columns: &[(String, String)],
        predicates: Vec<Expr>,
        group_expr: Vec<Expr>,
        aggregate_expr: Vec<Expr>,
        limit: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        if limit == 0 || limit > self.limits.max_rows.saturating_add(1) {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "authorized aggregate plan exceeds its configured row limit"
            ))
            .into());
        }
        let mut frame = self
            .context
            .table(relation)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        for (input_column, output_column) in unnest_columns {
            frame = frame
                .unnest_columns_with_options(
                    &[input_column.as_str()],
                    datafusion::common::UnnestOptions::new().with_recursions(
                        datafusion::common::RecursionUnnestOption {
                            input_column: input_column.clone().into(),
                            output_column: output_column.clone().into(),
                            depth: 1,
                        },
                    ),
                )
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        for predicate in predicates {
            frame = frame
                .filter(predicate)
                .map_err(AuthorizedQueryError::execution_failed)?;
        }
        let frame = frame
            .aggregate(group_expr, aggregate_expr)
            .map_err(AuthorizedQueryError::execution_failed)?
            .limit(0, Some(limit))
            .map_err(AuthorizedQueryError::invalid_query)?;
        self.execute_frame(frame, limit).await
    }

    /// Runs one aggregate over the union of authorized relation views. This
    /// keeps cross-Form statistics inside DataFusion so the final Rust decode
    /// sees only the globally bounded aggregate result.
    pub(crate) async fn execute_union_relation_aggregate_plan(
        &self,
        relations: &[String],
        unnest_columns: &[(String, String)],
        group_expr: Vec<Expr>,
        aggregate_expr: Vec<Expr>,
        limit: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        if relations.is_empty() || limit == 0 || limit > self.limits.max_rows.saturating_add(1) {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "authorized aggregate plan exceeds its configured row limit"
            ))
            .into());
        }
        let mut unioned: Option<DataFrame> = None;
        for relation in relations {
            let mut frame = self
                .context
                .table(relation)
                .await
                .map_err(AuthorizedQueryError::execution_failed)?;
            for (input_column, output_column) in unnest_columns {
                frame = frame
                    .unnest_columns_with_options(
                        &[input_column.as_str()],
                        datafusion::common::UnnestOptions::new().with_recursions(
                            datafusion::common::RecursionUnnestOption {
                                input_column: input_column.clone().into(),
                                output_column: output_column.clone().into(),
                                depth: 1,
                            },
                        ),
                    )
                    .map_err(AuthorizedQueryError::execution_failed)?;
            }
            let projection = unnest_columns
                .iter()
                .map(|(_, output_column)| col(output_column).alias(output_column))
                .collect::<Vec<_>>();
            frame = frame
                .select(projection)
                .map_err(AuthorizedQueryError::execution_failed)?;
            unioned = Some(match unioned {
                None => frame,
                Some(previous) => previous
                    .union(frame)
                    .map_err(AuthorizedQueryError::execution_failed)?,
            });
        }
        let frame = unioned
            .expect("non-empty relation list produces a unioned DataFrame")
            .aggregate(group_expr, aggregate_expr)
            .map_err(AuthorizedQueryError::execution_failed)?
            .limit(0, Some(limit))
            .map_err(AuthorizedQueryError::invalid_query)?;
        self.execute_frame(frame, limit).await
    }

    async fn execute_frame(
        &self,
        frame: DataFrame,
        limit: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_frame_with_byte_limit(frame, limit, None).await
    }

    async fn execute_frame_with_byte_limit(
        &self,
        frame: DataFrame,
        limit: usize,
        max_bytes: Option<usize>,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(self.limits.timeout, async {
            let plan = self
                .context
                .state()
                .optimize(&frame.logical_plan().clone())
                .map_err(AuthorizedQueryError::invalid_query)?;
            validate_logical_plan(&plan, &self.authorized_relations)
                .map_err(AuthorizedQueryError::unauthorized)?;
            let validation_plan = plan.clone();
            let frame = self
                .context
                .execute_logical_plan(plan)
                .await
                .map_err(AuthorizedQueryError::execution_failed)?;
            let batches = match max_bytes {
                Some(max_bytes) => self.collect_frame_bounded(frame, max_bytes).await?,
                None => self.collect_frame(frame).await?,
            };
            let rows = batches.iter().map(|batch| batch.num_rows()).sum::<usize>();
            if rows > self.limits.max_rows || rows > limit {
                return Err(AuthorizedQueryError::resource_limit(anyhow!(
                    "authorized query row limit exceeded"
                ))
                .into());
            }
            self.validate_revision_invariants(&validation_plan).await?;
            Ok(batches)
        })
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Evaluates a nested Struct value in a Form-owned list without exposing
    /// the SessionContext. This keeps list-reference checks inside the same
    /// closed, Entry-scoped DataFusion boundary as scalar checks.
    pub async fn contains_struct_list_value(
        &self,
        relation: &str,
        list_field: &str,
        child_field: &str,
        expected: &str,
    ) -> Result<bool> {
        let batches = self
            .execute_relation_plan(
                relation,
                &[(list_field.to_string(), "__ugoite_unnested_item".to_string())],
                vec![datafusion::functions::core::expr_fn::get_field(
                    col("__ugoite_unnested_item"),
                    child_field,
                )
                .eq(lit(expected))],
                vec![lit(1).alias("__ugoite_match")],
                Vec::new(),
                false,
                false,
                1,
            )
            .await?;
        Ok(batches.iter().any(|batch| batch.num_rows() > 0))
    }

    /// Parses a statement through the same closed DataFusion context used for
    /// execution and returns its native named placeholders.
    pub async fn parameter_names(&self, sql: &str) -> Result<BTreeSet<String>> {
        Ok(self
            .context
            .state()
            .create_logical_plan(sql)
            .await
            .map_err(AuthorizedQueryError::invalid_query)?
            .get_parameter_names()
            .map_err(AuthorizedQueryError::invalid_query)?
            .into_iter()
            .collect())
    }

    /// Validates a session query against this closed context without executing
    /// it. This binds native parameters, resolves only authorized relations,
    /// and applies the same read-only plan checks used at execution time.
    pub async fn validate_with_parameters(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<()> {
        self.prepared_plan(sql, parameters).await.map(|_| ())
    }

    /// Plans and executes a read-only statement. User predicates are evaluated
    /// above the trusted Entry filter embedded in every registered view.
    pub async fn execute(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_with_parameters(sql, HashMap::new()).await
    }

    /// Executes a read-only statement with a pre-JSON Arrow materialization
    /// bound. This is used by the search candidate path, where a row limit
    /// does not constrain the size of user-controlled string columns.
    pub(crate) async fn execute_with_byte_limit(
        &self,
        sql: &str,
        max_bytes: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_with_permit_and_byte_limit(sql, HashMap::new(), Some(max_bytes)),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Binds DataFusion-native `$name` placeholders after parsing and before
    /// optimization. Values never become SQL text, so quotes and SQL-looking
    /// strings retain their scalar meaning.
    pub async fn execute_with_parameters(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_with_permit(sql, parameters),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Executes one stateless SQL page without computing a total count. The
    /// offset is a continuation coordinate, not a result-window limit; only
    /// the materialized page is bounded by the query resource policy.
    pub async fn execute_stateless_page(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<StatelessSqlPage> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_stateless_page_with_permit(sql, parameters, offset, limit),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Executes the explicit count operation for stateless SQL. It is kept
    /// separate from page execution so a normal page never performs an
    /// arbitrary count wrapper.
    pub async fn execute_stateless_count(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<u64> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_stateless_count_with_permit(sql, parameters),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn physical_plan_for_testing(&self, sql: &str) -> Result<String> {
        let plan = self.context.state().create_logical_plan(sql).await?;
        validate_logical_plan(&plan, &self.authorized_relations)?;
        let optimized = self.context.state().optimize(&plan)?;
        validate_logical_plan(&optimized, &self.authorized_relations)?;
        let frame = self.context.execute_logical_plan(optimized).await?;
        let physical = frame.create_physical_plan().await?;
        validate_physical_plan(&physical, &self.authorized_scans)?;
        Ok(format!("{physical:?}"))
    }

    async fn execute_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_with_permit_and_byte_limit(sql, parameters, None)
            .await
    }

    async fn execute_with_permit_and_byte_limit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        max_bytes: Option<usize>,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let plan = self.prepared_plan(sql, parameters).await?;
        let validation_plan = plan.clone();
        let frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let max_rows_with_sentinel = self
            .limits
            .max_rows
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorized query row limit is too large"))
            .map_err(AuthorizedQueryError::resource_limit)?;
        let frame = frame
            .limit(0, Some(max_rows_with_sentinel))
            .map_err(AuthorizedQueryError::resource_limit)?;
        let batches = match max_bytes {
            Some(max_bytes) => self.collect_frame_bounded(frame, max_bytes).await?,
            None => self.collect_frame(frame).await?,
        };
        let rows = batches.iter().map(|batch| batch.num_rows()).sum::<usize>();
        if rows > self.limits.max_rows {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "authorized query row limit exceeded"
            ))
            .into());
        }
        // Validate after materializing the statement, but before returning
        // any rows. Iceberg readers can observe a newer committed manifest
        // between planning and collection; validating this point prevents a
        // duplicate maximum revision from escaping in that interval.
        self.validate_revision_invariants(&validation_plan).await?;
        Ok(batches)
    }

    async fn execute_stateless_page_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<StatelessSqlPage> {
        if limit == 0 || limit > self.limits.max_rows {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "SQL query page exceeds its configured row limit"
            ))
            .into());
        }
        let plan = self.prepared_plan(sql, parameters).await?;
        let (columns, result_schema) = sql_output_shape(&plan)?;
        let has_order = stateless_query_has_top_level_order(sql)?;
        let validation_plan = plan.clone();
        let frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let page = frame
            .limit(offset, Some(limit))
            .map_err(AuthorizedQueryError::resource_limit)?;
        let batches = self.collect_frame(page).await?;
        self.validate_revision_invariants(&validation_plan).await?;
        Ok(StatelessSqlPage {
            columns,
            result_schema,
            batches,
            has_order,
        })
    }

    async fn execute_stateless_count_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<u64> {
        let plan = self.prepared_plan(sql, parameters).await?;
        let validation_plan = plan.clone();
        let frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_frame = frame
            .aggregate(
                Vec::new(),
                vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                    .alias("ugoite_query_count")],
            )
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_batches = self.collect_frame(count_frame).await?;
        self.validate_revision_invariants(&validation_plan).await?;
        count_from_batches(&count_batches)
    }

    async fn prepared_plan(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<LogicalPlan> {
        let plan = self.bound_logical_plan(sql, parameters).await?;
        let optimized = self
            .context
            .state()
            .optimize(&plan)
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(&optimized, &self.authorized_relations)
            .map_err(AuthorizedQueryError::unauthorized)?;
        Ok(optimized)
    }

    async fn bound_logical_plan(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<LogicalPlan> {
        let sql = resolve_form_name_references(sql, &self.form_name_aliases)?;
        let plan = self
            .context
            .state()
            .create_logical_plan(&sql)
            .await
            .map_err(AuthorizedQueryError::invalid_query)?;
        let expected = plan
            .get_parameter_names()
            .map_err(AuthorizedQueryError::invalid_query)?
            .into_iter()
            .map(|name| name.trim_start_matches('$').to_string())
            .collect::<BTreeSet<_>>();
        let supplied = parameters.keys().cloned().collect::<BTreeSet<_>>();
        if expected != supplied {
            return Err(AuthorizedQueryError::invalid_query(anyhow!(
                "SQL parameters do not exactly match DataFusion placeholders"
            ))
            .into());
        }
        let plan = plan
            .with_param_values(parameters)
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(&plan, &self.authorized_relations)
            .map_err(AuthorizedQueryError::unauthorized)?;
        Ok(plan)
    }

    async fn collect_frame(&self, frame: DataFrame) -> Result<Vec<arrow_array::RecordBatch>> {
        let task_context = Arc::new(frame.task_ctx());
        let physical = frame
            .create_physical_plan()
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        validate_physical_plan(&physical, &self.authorized_scans)
            .map_err(AuthorizedQueryError::unauthorized)?;
        let stream = datafusion::physical_plan::execute_stream(physical, task_context)
            .map_err(classify_datafusion_error)?;
        collect_query_stream(stream).await
    }

    async fn collect_frame_bounded(
        &self,
        frame: DataFrame,
        max_bytes: usize,
    ) -> Result<Vec<arrow_array::RecordBatch>> {
        let task_context = Arc::new(frame.task_ctx());
        let physical = frame
            .create_physical_plan()
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        validate_physical_plan(&physical, &self.authorized_scans)
            .map_err(AuthorizedQueryError::unauthorized)?;
        let stream = datafusion::physical_plan::execute_stream(physical, task_context)
            .map_err(classify_datafusion_error)?;
        let mut batches = Vec::new();
        let mut bytes = 0usize;
        let _active_stream = ActiveQueryStream::new(None);
        let mut stream = stream;
        while let Some(batch) = futures::TryStreamExt::try_next(&mut stream)
            .await
            .map_err(classify_datafusion_error)?
        {
            bytes = bytes
                .checked_add(batch.get_array_memory_size())
                .ok_or_else(|| {
                    AuthorizedQueryError::resource_limit(anyhow!(
                        "authorized query materialization exceeds its byte limit"
                    ))
                })?;
            if bytes > max_bytes {
                return Err(AuthorizedQueryError::resource_limit(anyhow!(
                    "authorized query materialization exceeds its byte limit"
                ))
                .into());
            }
            batches.push(batch);
        }
        Ok(batches)
    }

    async fn validate_revision_invariants(&self, plan: &LogicalPlan) -> Result<()> {
        let mut scanned_providers = BTreeSet::new();
        collect_scanned_provider_addresses(plan, &mut scanned_providers);
        let mut validated = self.duplicate_head_checks_validated.lock().await;
        for (provider, check) in &self.duplicate_head_checks {
            let provider_address = Arc::as_ptr(provider) as *const () as usize;
            if !scanned_providers.contains(&provider_address) {
                continue;
            }
            if validated.contains(&provider_address) {
                continue;
            }
            if self
                .collect_frame(check.clone())
                .await?
                .iter()
                .any(|batch| batch.num_rows() > 0)
            {
                return Err(AuthorizedQueryError::RevisionInvariantViolation.into());
            }
            validated.insert(provider_address);
        }
        Ok(())
    }
}

fn collect_scanned_provider_addresses(plan: &LogicalPlan, providers: &mut BTreeSet<usize>) {
    if let LogicalPlan::TableScan(scan) = plan {
        if let Some(source) =
            (scan.source.as_ref() as &dyn Any).downcast_ref::<DefaultTableSource>()
        {
            providers.insert(Arc::as_ptr(&source.table_provider) as *const () as usize);
        }
    }
    for input in plan.inputs() {
        collect_scanned_provider_addresses(input, providers);
    }
}

/// One bounded stateless SQL page with its server-owned output shape.
///
/// `result_schema` mirrors `columns` positionally from the planned output
/// schema, so null and empty results keep their types without the Browser
/// inferring types from row values.
pub struct StatelessSqlPage {
    pub columns: Vec<String>,
    pub result_schema: Vec<SqlResultColumn>,
    pub batches: Vec<arrow_array::RecordBatch>,
    pub has_order: bool,
}

/// Map one Arrow output type to its portable logical column type.
///
/// Only exact portable equivalents map to a scalar type; every other
/// output (nested, binary, temporal-without-date-semantics, decimal, null,
/// or dictionary-encoded values) stays `json` so the transported rows keep
/// their exact values and later metric authoring rejects ambiguity instead
/// of guessing.
pub(crate) fn sql_result_column_type(
    data_type: &datafusion::arrow::datatypes::DataType,
) -> SqlResultColumnType {
    use datafusion::arrow::datatypes::DataType as Arrow;
    match data_type {
        Arrow::Boolean => SqlResultColumnType::Boolean,
        Arrow::Int8
        | Arrow::Int16
        | Arrow::Int32
        | Arrow::Int64
        | Arrow::UInt8
        | Arrow::UInt16
        | Arrow::UInt32
        | Arrow::UInt64 => SqlResultColumnType::Integer,
        Arrow::Float16 | Arrow::Float32 | Arrow::Float64 => SqlResultColumnType::Float,
        Arrow::Utf8 | Arrow::LargeUtf8 | Arrow::Utf8View => SqlResultColumnType::String,
        Arrow::Date32 | Arrow::Date64 => SqlResultColumnType::Date,
        Arrow::Timestamp(_, _) => SqlResultColumnType::Timestamp,
        _ => SqlResultColumnType::Json,
    }
}

fn sql_output_shape(plan: &LogicalPlan) -> Result<(Vec<String>, Vec<SqlResultColumn>)> {
    let fields = plan.schema().fields();
    if fields.len() > MAX_SQL_OUTPUT_COLUMNS {
        return Err(AuthorizedQueryError::resource_limit(anyhow!(
            "SQL output contains too many columns"
        ))
        .into());
    }
    let mut metadata_bytes = 0usize;
    let mut columns = Vec::with_capacity(fields.len());
    let mut result_schema = Vec::with_capacity(fields.len());
    for field in fields {
        let name = field.name().to_string();
        if name.len() > MAX_SQL_COLUMN_NAME_BYTES {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "SQL output column name exceeds its byte limit"
            ))
            .into());
        }
        metadata_bytes = metadata_bytes.checked_add(name.len()).ok_or_else(|| {
            AuthorizedQueryError::resource_limit(anyhow!(
                "SQL output column metadata exceeds its byte limit"
            ))
        })?;
        if metadata_bytes > MAX_SQL_COLUMN_METADATA_BYTES {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "SQL output column metadata exceeds its byte limit"
            ))
            .into());
        }
        result_schema.push(SqlResultColumn {
            name: name.clone(),
            column_type: sql_result_column_type(field.data_type()),
        });
        columns.push(name);
    }
    Ok((columns, result_schema))
}

fn stateless_query_has_top_level_order(sql: &str) -> Result<bool> {
    use datafusion::sql::parser::{DFParser, Statement as DataFusionStatement};
    use datafusion::sql::sqlparser::ast::Statement as SqlStatement;

    let statements = DFParser::parse_sql(sql).map_err(AuthorizedQueryError::invalid_query)?;
    let Some(DataFusionStatement::Statement(statement)) = statements.front() else {
        return Err(AuthorizedQueryError::invalid_query(anyhow!(
            "SQL query must contain one SELECT statement"
        ))
        .into());
    };
    let SqlStatement::Query(query) = statement.as_ref() else {
        return Err(AuthorizedQueryError::invalid_query(anyhow!(
            "SQL query must contain one SELECT statement"
        ))
        .into());
    };
    Ok(query.order_by.is_some())
}

fn count_from_batches(batches: &[arrow_array::RecordBatch]) -> Result<u64> {
    let batch = batches
        .iter()
        .find(|batch| batch.num_rows() == 1)
        .ok_or_else(|| AuthorizedQueryError::execution_failed(anyhow!("count returned no row")))?;
    let values = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Int64Array>()
        .ok_or_else(|| AuthorizedQueryError::execution_failed(anyhow!("count has invalid type")))?;
    u64::try_from(values.value(0))
        .map_err(|error| AuthorizedQueryError::execution_failed(anyhow!(error)).into())
}

struct VisibleColumn {
    source: String,
    name: String,
}

fn visible_columns(
    form: &ugoite_domain::form::FormDefinition,
    policy: &ugoite_core::query::AuthorizedQueryForm,
    schema: &datafusion::arrow::datatypes::Schema,
) -> Result<Vec<VisibleColumn>> {
    let form_columns = form
        .fields
        .iter()
        .map(|field| (sql_column_name(field.id), field.name.as_str()))
        .collect::<HashMap<_, _>>();
    let mut visible = policy
        .columns
        .iter()
        .map(|column| {
            let source = form_columns.get(column).ok_or_else(|| {
                anyhow!("authorized query policy exposes unknown Form column {column}")
            })?;
            Ok(VisibleColumn {
                source: (*source).to_string(),
                name: column.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    visible.extend(policy.system_columns.iter().map(system_column));
    let claimed_sources = visible
        .iter()
        .map(|column| column.source.clone())
        .collect::<BTreeSet<_>>();
    for field in schema.fields() {
        let source = field.name();
        if claimed_sources.contains(source)
            || form_columns.contains_key(source.as_str())
            || is_internal_revision_column(source)
        {
            continue;
        }
        // Physical columns that are not claimed by the current Form/system
        // schema remain readable by their physical name. They are never
        // writable or assigned an Entry-level meaning.
        visible.push(VisibleColumn {
            source: source.clone(),
            name: source.clone(),
        });
    }
    let mut exposed = BTreeSet::new();
    if visible
        .iter()
        .any(|column| !exposed.insert(column.name.clone()))
    {
        bail!("authorized query policy exposes duplicate column names");
    }
    if visible.is_empty() {
        bail!(
            "authorized query policy exposes no columns for {}",
            policy.relation
        );
    }
    Ok(visible)
}

fn is_internal_revision_column(name: &str) -> bool {
    matches!(
        name,
        "entry_id"
            | "entry_version"
            | "operation"
            | "committed_at"
            | "revision_id"
            | "parent_revision_id"
            | "author_id"
            | "extra_attributes"
    )
}

fn system_column(column: &QuerySystemColumn) -> VisibleColumn {
    let (source, name) = match column {
        QuerySystemColumn::ExternalId => ("ugoite_entry_external_id", "_ugoite_id"),
        QuerySystemColumn::Tags => ("ugoite_entry_tags", "_ugoite_tags"),
        QuerySystemColumn::CreatedAt => ("ugoite_entry_created_at", "_ugoite_created_at"),
        QuerySystemColumn::UpdatedAt => ("ugoite_entry_updated_at", "_ugoite_updated_at"),
        QuerySystemColumn::EntryId => ("entry_id", "_ugoite_entry_id"),
        QuerySystemColumn::EntryVersion => ("entry_version", "_ugoite_entry_version"),
        QuerySystemColumn::CommittedAt => ("committed_at", "_ugoite_committed_at"),
        QuerySystemColumn::RevisionId => ("revision_id", "_ugoite_revision_id"),
        QuerySystemColumn::ParentRevisionId => ("parent_revision_id", "_ugoite_parent_revision_id"),
        QuerySystemColumn::Author => ("author_id", "_ugoite_author"),
        QuerySystemColumn::UpdatedBy => ("ugoite_entry_updated_by", "_ugoite_updated_by"),
        QuerySystemColumn::DeletedBy => ("ugoite_entry_deleted_by", "_ugoite_deleted_by"),
        QuerySystemColumn::ExtraAttributes => ("extra_attributes", "_ugoite_extra_attributes"),
        QuerySystemColumn::Integrity => ("ugoite_entry_integrity", "_ugoite_integrity"),
        QuerySystemColumn::Deleted => ("ugoite_entry_deleted", "_ugoite_deleted"),
        QuerySystemColumn::DeletedAt => ("ugoite_entry_deleted_at", "_ugoite_deleted_at"),
    };
    VisibleColumn {
        source: source.to_string(),
        name: name.to_string(),
    }
}

fn validate_relation(relation: &str) -> Result<()> {
    if relation.starts_with(INTERNAL_RELATION_PREFIX) {
        bail!("authorized query relation uses a reserved internal prefix");
    }
    let mut characters = relation.chars();
    let Some(first) = characters.next() else {
        bail!("authorized query relation must not be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        bail!("authorized query relation must be an ASCII SQL identifier");
    }
    Ok(())
}

fn validate_form_name(name: &str) -> Result<()> {
    ugoite_domain::id::validate_form_name(name)
        .map_err(|error| anyhow!("invalid SQL Form name {name}: {error}"))
}

fn is_legacy_relation_name(name: &str) -> bool {
    let Some((prefix, id)) = name.split_at_checked("form_".len()) else {
        return false;
    };
    prefix.eq_ignore_ascii_case("form_")
        && id.len() == 32
        && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn resolve_form_name_references(sql: &str, aliases: &BTreeMap<String, String>) -> Result<String> {
    resolve_form_name_references_with_used(sql, aliases).map(|(sql, _)| sql)
}

pub(crate) fn resolve_form_name_references_with_used(
    sql: &str,
    aliases: &BTreeMap<String, String>,
) -> Result<(String, BTreeSet<String>)> {
    let (sql, used, _) = resolve_form_name_references_with_references(sql, aliases)?;
    Ok((sql, used))
}

pub(crate) fn collect_quoted_form_name_references(sql: &str) -> Result<BTreeSet<String>> {
    resolve_form_name_references_with_references(sql, &BTreeMap::new()).map(|(_, _, refs)| refs)
}

fn resolve_form_name_references_with_references(
    sql: &str,
    aliases: &BTreeMap<String, String>,
) -> Result<(String, BTreeSet<String>, BTreeSet<String>)> {
    use datafusion::sql::parser::DFParser;
    use datafusion::sql::sqlparser::ast::{
        Ident, ObjectName, ObjectNamePart, VisitMut, VisitorMut,
    };
    use std::ops::ControlFlow;

    struct AliasReferenceResolver<'a> {
        aliases: &'a BTreeMap<String, String>,
        cte_scopes: Vec<BTreeSet<String>>,
        error: Option<anyhow::Error>,
        changed: bool,
        used: BTreeSet<String>,
        references: BTreeSet<String>,
    }

    impl VisitorMut for AliasReferenceResolver<'_> {
        type Break = ();

        fn pre_visit_query(
            &mut self,
            query: &mut datafusion::sql::sqlparser::ast::Query,
        ) -> ControlFlow<Self::Break> {
            self.cte_scopes.push(
                query
                    .with
                    .as_ref()
                    .into_iter()
                    .flat_map(|with| with.cte_tables.iter())
                    .map(|cte| {
                        let identifier = &cte.alias.name;
                        if identifier.quote_style.is_some() {
                            identifier.value.clone()
                        } else {
                            identifier.value.to_ascii_lowercase()
                        }
                    })
                    .collect(),
            );
            ControlFlow::Continue(())
        }

        fn post_visit_query(
            &mut self,
            _query: &mut datafusion::sql::sqlparser::ast::Query,
        ) -> ControlFlow<Self::Break> {
            self.cte_scopes.pop();
            ControlFlow::Continue(())
        }

        fn pre_visit_relation(&mut self, relation: &mut ObjectName) -> ControlFlow<Self::Break> {
            let Some(identifier) = relation.0.last().and_then(|part| part.as_ident()) else {
                return ControlFlow::Continue(());
            };
            let name = identifier.value.as_str();
            let normalized_name = if identifier.quote_style.is_some() {
                name.to_string()
            } else {
                name.to_ascii_lowercase()
            };
            if relation.0.len() == 1
                && self
                    .cte_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains(&normalized_name))
            {
                return ControlFlow::Continue(());
            }
            if relation.0.len() == 1
                && identifier.quote_style == Some('"')
                && !is_legacy_relation_name(name)
            {
                self.references.insert(name.to_string());
            }
            let case_match = self
                .aliases
                .keys()
                .any(|alias| alias.eq_ignore_ascii_case(name));
            if !case_match {
                return ControlFlow::Continue(());
            }
            if relation.0.len() != 1
                || identifier.quote_style != Some('"')
                || !self.aliases.contains_key(name)
            {
                self.error = Some(anyhow!(
                    "Form name {name} must be referenced as a double-quoted SQL relation"
                ));
                return ControlFlow::Break(());
            }
            let resolved = self.aliases.get(name).expect("checked alias above");
            self.used.insert(name.to_string());
            relation.0 = vec![ObjectNamePart::Identifier(Ident::new(resolved))];
            self.changed = true;
            ControlFlow::Continue(())
        }
    }

    let mut statements = DFParser::parse_sql(sql).map_err(AuthorizedQueryError::invalid_query)?;
    let mut resolver = AliasReferenceResolver {
        aliases,
        cte_scopes: Vec::new(),
        error: None,
        changed: false,
        used: BTreeSet::new(),
        references: BTreeSet::new(),
    };
    for statement in &mut statements {
        let datafusion::sql::parser::Statement::Statement(statement) = statement else {
            continue;
        };
        if statement.as_mut().visit(&mut resolver).is_break() {
            break;
        }
    }
    if let Some(error) = resolver.error {
        return Err(AuthorizedQueryError::invalid_query(error).into());
    }
    if !resolver.changed {
        return Ok((sql.to_string(), resolver.used, resolver.references));
    }
    Ok((
        statements
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(";\n"),
        resolver.used,
        resolver.references,
    ))
}

fn validate_logical_plan(
    plan: &LogicalPlan,
    authorized_relations: &BTreeSet<String>,
) -> Result<()> {
    // The private catalog and view/provider construction are the authorization
    // boundary: a plan cannot resolve the hidden Iceberg source directly, and
    // every public relation already contains its Entry predicate and visible
    // projection. Do not attempt to re-evaluate SQL predicate semantics here;
    // this defense-in-depth check is deliberately limited to statement kinds
    // and the relations the planner resolved.
    match plan {
        LogicalPlan::Explain(_) | LogicalPlan::Analyze(_) => {
            bail!("EXPLAIN is not supported for read-only query execution")
        }
        LogicalPlan::Dml(_)
        | LogicalPlan::Ddl(_)
        | LogicalPlan::Copy(_)
        | LogicalPlan::Statement(_)
        | LogicalPlan::DescribeTable(_)
        | LogicalPlan::Extension(_)
        | LogicalPlan::RecursiveQuery(_) => {
            bail!("statement kind is not supported for read-only query execution")
        }
        LogicalPlan::TableScan(scan) => {
            let relation = scan.table_name.to_string();
            if !authorized_relations.contains(&relation) {
                bail!("query plan scans an unauthorized relation {relation}");
            }
        }
        LogicalPlan::Projection(_)
        | LogicalPlan::Filter(_)
        | LogicalPlan::Window(_)
        | LogicalPlan::Aggregate(_)
        | LogicalPlan::Sort(_)
        | LogicalPlan::Join(_)
        | LogicalPlan::Repartition(_)
        | LogicalPlan::Union(_)
        | LogicalPlan::EmptyRelation(_)
        | LogicalPlan::Subquery(_)
        | LogicalPlan::SubqueryAlias(_)
        | LogicalPlan::Limit(_)
        | LogicalPlan::Values(_)
        | LogicalPlan::Distinct(_)
        | LogicalPlan::Unnest(_) => {}
    }
    for input in plan.inputs() {
        validate_logical_plan(input, authorized_relations)?;
    }
    Ok(())
}

fn validate_physical_plan(
    plan: &Arc<dyn ExecutionPlan>,
    authorized_scans: &BTreeSet<AuthorizedScan>,
) -> Result<()> {
    if let Some(scan) = (plan.as_ref() as &dyn Any)
        .downcast_ref::<iceberg_datafusion::physical_plan::IcebergTableScan>()
    {
        let authorized = AuthorizedScan {
            table_uuid: scan.table().metadata().uuid().to_string(),
            snapshot_id: scan.snapshot_id(),
        };
        if !authorized_scans.contains(&authorized) {
            bail!("physical plan scans an unauthorized Iceberg table");
        }
    } else if plan.children().is_empty() && plan.name() != "EmptyExec" {
        // Intermediate DataFusion operators are not an authorization boundary.
        // A leaf is: permit only an authorized Iceberg scan (or an empty plan)
        // so a future external provider cannot silently enter the query.
        bail!("physical plan has an unauthorized data source");
    }
    for child in plan.children() {
        validate_physical_plan(child, authorized_scans)?;
    }
    Ok(())
}

#[cfg(test)]
mod cancellation_tests {
    use super::collect_query_stream_with_probe;
    use arrow_schema::Schema;
    use datafusion::error::DataFusionError;
    use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
    use futures::Stream;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};

    struct SlowSource {
        dropped: Arc<AtomicBool>,
        started: Arc<tokio::sync::Notify>,
        first_poll: bool,
    }

    impl Stream for SlowSource {
        type Item = Result<arrow_array::RecordBatch, DataFusionError>;

        fn poll_next(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            if self.first_poll {
                self.first_poll = false;
                self.started.notify_one();
            }
            Poll::Pending
        }
    }

    impl Drop for SlowSource {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn aborting_query_collector_drops_slow_source_and_clears_active_count() {
        let dropped = Arc::new(AtomicBool::new(false));
        let query_active = Arc::new(AtomicBool::new(false));
        let started = Arc::new(tokio::sync::Notify::new());
        let stream = Box::pin(RecordBatchStreamAdapter::new(
            Arc::new(Schema::empty()),
            SlowSource {
                dropped: dropped.clone(),
                started: started.clone(),
                first_poll: true,
            },
        ));
        let query = tokio::spawn(collect_query_stream_with_probe(
            stream,
            Some(query_active.clone()),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .expect("slow source must be polled");
        assert!(
            query_active.load(Ordering::SeqCst),
            "query stream did not become active"
        );

        query.abort();
        assert!(query
            .await
            .expect_err("aborted query must not finish")
            .is_cancelled());

        assert!(
            dropped.load(Ordering::SeqCst),
            "DataFusion source was not dropped"
        );
        assert!(
            !query_active.load(Ordering::SeqCst),
            "query active guard leaked"
        );
    }
}

#[cfg(test)]
mod result_schema_tests {
    use super::sql_result_column_type;
    use datafusion::arrow::datatypes::{DataType, Field, TimeUnit};
    use std::sync::Arc;
    use ugoite_core::sql_query::SqlResultColumnType;

    #[test]
    fn arrow_output_types_map_to_portable_result_types() {
        let cases = [
            (DataType::Boolean, SqlResultColumnType::Boolean),
            (DataType::Int8, SqlResultColumnType::Integer),
            (DataType::Int16, SqlResultColumnType::Integer),
            (DataType::Int32, SqlResultColumnType::Integer),
            (DataType::Int64, SqlResultColumnType::Integer),
            (DataType::UInt8, SqlResultColumnType::Integer),
            (DataType::UInt16, SqlResultColumnType::Integer),
            (DataType::UInt32, SqlResultColumnType::Integer),
            (DataType::UInt64, SqlResultColumnType::Integer),
            (DataType::Float16, SqlResultColumnType::Float),
            (DataType::Float32, SqlResultColumnType::Float),
            (DataType::Float64, SqlResultColumnType::Float),
            (DataType::Utf8, SqlResultColumnType::String),
            (DataType::LargeUtf8, SqlResultColumnType::String),
            (DataType::Utf8View, SqlResultColumnType::String),
            (DataType::Date32, SqlResultColumnType::Date),
            (DataType::Date64, SqlResultColumnType::Date),
            (
                DataType::Timestamp(TimeUnit::Microsecond, None),
                SqlResultColumnType::Timestamp,
            ),
            (
                DataType::Timestamp(TimeUnit::Nanosecond, Some(Arc::from("+00:00"))),
                SqlResultColumnType::Timestamp,
            ),
        ];
        for (arrow, portable) in cases {
            assert_eq!(sql_result_column_type(&arrow), portable, "{arrow:?}");
        }
    }

    #[test]
    fn non_scalar_arrow_outputs_stay_json_without_inference() {
        // Nested, binary, time-of-day, interval, decimal, null, and
        // dictionary outputs keep their exact row values as json instead of
        // guessing a scalar portable type from the physical representation.
        let cases = [
            DataType::Null,
            DataType::Binary,
            DataType::LargeBinary,
            DataType::Time32(datafusion::arrow::datatypes::TimeUnit::Second),
            DataType::Time64(datafusion::arrow::datatypes::TimeUnit::Microsecond),
            DataType::Duration(datafusion::arrow::datatypes::TimeUnit::Millisecond),
            DataType::Interval(datafusion::arrow::datatypes::IntervalUnit::MonthDayNano),
            DataType::Decimal128(10, 2),
            DataType::List(Arc::new(Field::new("item", DataType::Int32, true))),
            DataType::Struct(vec![Field::new("a", DataType::Int32, true)].into()),
            DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
        ];
        for arrow in cases {
            assert_eq!(
                sql_result_column_type(&arrow),
                SqlResultColumnType::Json,
                "{arrow:?}"
            );
        }
    }
}

#[cfg(test)]
mod form_name_resolution_tests {
    use super::{is_legacy_relation_name, resolve_form_name_references};
    use std::collections::BTreeMap;

    #[test]
    fn resolves_quoted_form_names_in_nested_sql_ast_relations() {
        let bindings = BTreeMap::from([("Expense-2026".to_string(), "form_1234".to_string())]);
        let sql = "WITH recent AS (SELECT * FROM \"Expense-2026\") \
                   SELECT 'Expense-2026' AS label FROM recent \
                   JOIN (SELECT * FROM \"Expense-2026\") AS nested ON true";
        let resolved = resolve_form_name_references(sql, &bindings).unwrap();

        assert_eq!(resolved.matches("form_1234").count(), 2);
        assert!(resolved.contains("recent JOIN"));
        assert!(resolved.contains("'Expense-2026'"));
    }

    #[test]
    fn resolves_exact_case_distinct_and_digit_leading_form_names() {
        let bindings = BTreeMap::from([
            ("Expense".to_string(), "form_upper".to_string()),
            ("expense".to_string(), "form_lower".to_string()),
            ("2026-Expense".to_string(), "form_dated".to_string()),
        ]);
        let resolved = resolve_form_name_references(
            "SELECT * FROM \"Expense\" JOIN \"expense\" ON true \
             JOIN \"2026-Expense\" ON true",
            &bindings,
        )
        .unwrap();
        assert!(resolved.contains("form_upper"));
        assert!(resolved.contains("form_lower"));
        assert!(resolved.contains("form_dated"));
    }

    #[test]
    fn rejects_unquoted_or_wrong_case_form_names() {
        let bindings = BTreeMap::from([("Expense".to_string(), "form_1234".to_string())]);
        assert!(resolve_form_name_references("SELECT * FROM Expense", &bindings).is_err());
        assert!(resolve_form_name_references("SELECT * FROM \"expense\"", &bindings).is_err());
    }

    #[test]
    fn rejects_legacy_relation_shape_as_a_form_name() {
        assert!(is_legacy_relation_name(&format!("form_{}", "a".repeat(32))));
        assert!(is_legacy_relation_name(&format!("FORM_{}", "A".repeat(32))));
        assert!(!is_legacy_relation_name("form_not-a-uuid"));
    }
}
