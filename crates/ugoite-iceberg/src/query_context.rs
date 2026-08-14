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
use datafusion::logical_expr::{Expr, LogicalPlan, SortExpr};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::{col, lit, DataFrame, SessionConfig};
use iceberg_datafusion::IcebergStaticTableProvider;
use std::any::Any;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use ugoite_core::query::{AuthorizedQueryPolicy, EntryScope, QuerySystemColumn};
use ugoite_domain::form::sql_column_name;

use crate::{form_from_table, IcebergWorkspace};

const INTERNAL_RELATION_PREFIX: &str = "__ugoite_authorized_source_";

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
    let scoped = match entry_scope {
        EntryScope::AllCurrent => revisions,
        EntryScope::Only(entry_ids) if entry_ids.is_empty() => revisions.filter(lit(false))?,
        EntryScope::Only(entry_ids) => revisions.filter(
            col("entry_id").in_list(
                entry_ids
                    .iter()
                    .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
                false,
            ),
        )?,
        EntryScope::AllExcept(entry_ids) if entry_ids.is_empty() => revisions,
        EntryScope::AllExcept(entry_ids) => revisions.filter(
            col("entry_id").in_list(
                entry_ids
                    .iter()
                    .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                    .collect::<Vec<_>>(),
                true,
            ),
        )?,
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
    authorized_scans: BTreeSet<AuthorizedScan>,
    duplicate_head_checks: Vec<(Arc<dyn TableProvider>, DataFrame)>,
    duplicate_head_checks_validated: Arc<AsyncMutex<BTreeSet<usize>>>,
    // Cache the authorized latest-state logical plan, not its result.  Each
    // maintenance page adds its cursor and row limit before execution so a
    // large Form never becomes one unbounded Arrow allocation.
    latest_revision_cache: Arc<AsyncMutex<Option<LogicalPlan>>>,
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
        .with_scalar_functions(
            SessionStateDefaults::default_scalar_functions()
                .into_iter()
                .filter(|function| {
                    allowed_functions.contains(&function.name().to_ascii_lowercase())
                })
                .collect(),
        )
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
        let mut authorized_scans = BTreeSet::new();
        let mut duplicate_head_checks = Vec::new();

        if let Some(checkpoint) = &policy.checkpoint {
            self.validate_checkpoint(checkpoint)?;
            self.space_catalog
                .as_ref()
                .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
                .validate_checkpoint_evidence(checkpoint)
                .await?;
        }

        for (form_id, form_policy) in &policy.forms {
            validate_relation(&form_policy.relation)?;
            if !relations.insert(form_policy.relation.clone()) {
                bail!(
                    "authorized query policy repeats relation {}",
                    form_policy.relation
                );
            }
            let (form, table, expected_snapshot_id) = match &policy.checkpoint {
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
            let visible = visible_columns(&form, form_policy)?;
            let source = context.table(internal.as_str()).await?;
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
            context.register_table(form_policy.relation.as_str(), view)?;
        }

        let permits = self.shared_query_permits(policy.limits.max_concurrency);
        Ok(AuthorizedQueryContext {
            context,
            limits: policy.limits,
            permits,
            authorized_relations: relations,
            authorized_scans,
            duplicate_head_checks,
            duplicate_head_checks_validated: Arc::new(AsyncMutex::new(BTreeSet::new())),
            latest_revision_cache: Arc::new(AsyncMutex::new(None)),
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
            authorized_scans: BTreeSet::from([AuthorizedScan {
                table_uuid,
                snapshot_id,
            }]),
            duplicate_head_checks: vec![(provider, duplicate_head_check)],
            duplicate_head_checks_validated: Arc::new(AsyncMutex::new(BTreeSet::new())),
            latest_revision_cache: Arc::new(AsyncMutex::new(None)),
        })
    }
}

impl AuthorizedQueryContext {
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

    /// Executes one ordered page of the latest revision view. Maintenance
    /// readers use a keyset cursor so a large Form never becomes one giant
    /// revision-id `IN (...)` plan or one unbounded Arrow allocation.
    pub(crate) async fn execute_latest_revision_plan_page(
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
        let mut cache = self.latest_revision_cache.lock().await;
        if cache.is_none() {
            let source = self
                .context
                .table("revisions")
                .await
                .map_err(AuthorizedQueryError::execution_failed)?;
            let heads = latest_revision_dataframe(source, entry_scope, view)
                .map_err(AuthorizedQueryError::invalid_query)?
                .logical_plan()
                .clone();
            *cache = Some(heads);
        }
        let after = after_entry_id
            .map(uuid::Uuid::from_slice)
            .transpose()
            .map_err(AuthorizedQueryError::invalid_query)?;
        let plan = cache
            .as_ref()
            .expect("latest revision plan cache is set")
            .clone();
        drop(cache);
        let mut frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        if let Some(after) = after {
            frame = frame
                .filter(col("entry_id").gt(lit(after.as_bytes().to_vec())))
                .map_err(AuthorizedQueryError::invalid_query)?;
        }
        frame = frame
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
        self.execute_frame(frame, limit).await
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
            let batches = self.collect_frame(frame).await?;
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

    /// Validates the deliberately small SQL-session pagination contract at
    /// the same bound logical-plan stage used for execution. In particular,
    /// this is before optimizer rewrites can remove a sort from an empty or
    /// `LIMIT 0` plan.
    pub async fn validate_session_with_parameters(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<()> {
        self.prepared_session_plan(sql, parameters)
            .await
            .map(|_| ())
    }

    /// Plans and executes a read-only statement. User predicates are evaluated
    /// above the trusted Entry filter embedded in every registered view.
    pub async fn execute(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        self.execute_with_parameters(sql, HashMap::new()).await
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

    /// Executes a deterministic SQL session page and its count from the same
    /// authorized, checkpoint-pinned plan. Paging is a plan operation, never
    /// a Rust slice over materialized JSON rows.
    pub async fn execute_page(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<arrow_array::RecordBatch>, u64)> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_page_with_permit(sql, parameters, offset, limit),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Executes only the requested SQL-session page. The count endpoint uses
    /// [`Self::execute_session_count`] so it never performs an unrelated page
    /// execution.
    pub async fn execute_session_page(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<arrow_array::RecordBatch>, u64)> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(
            self.limits.timeout,
            self.execute_session_page_with_permit(sql, parameters, offset, limit),
        )
        .await
        .map_err(|_| AuthorizedQueryError::QueryTimedOut)?
    }

    /// Executes a count-only SQL-session plan from the frozen checkpoint.
    pub async fn execute_session_count(
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
            self.execute_session_count_with_permit(sql, parameters),
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
        let batches = self.collect_frame(frame).await?;
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

    async fn execute_page_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<arrow_array::RecordBatch>, u64)> {
        let requested_rows = offset
            .checked_add(limit)
            .ok_or_else(|| anyhow!("SQL session page range overflows"))
            .map_err(AuthorizedQueryError::resource_limit)?;
        if limit == 0 || limit > self.limits.max_rows || requested_rows > self.limits.max_rows {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "SQL session page exceeds its configured row limit"
            ))
            .into());
        }
        let plan = self.prepared_plan(sql, parameters).await?;
        let validation_plan = plan.clone();
        if !logical_plan_contains_sort(&plan) {
            return Err(AuthorizedQueryError::invalid_query(anyhow!(
                "SQL session paging requires an explicit ORDER BY"
            ))
            .into());
        }
        let frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_frame = frame
            .clone()
            .aggregate(
                Vec::new(),
                vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                    .alias("ugoite_session_count")],
            )
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_batches = self.collect_frame(count_frame).await?;
        let total = count_from_batches(&count_batches)?;
        let page = frame
            .limit(offset, Some(limit))
            .map_err(AuthorizedQueryError::resource_limit)?;
        let batches = self.collect_frame(page).await?;
        self.validate_revision_invariants(&validation_plan).await?;
        Ok((batches, total))
    }

    async fn execute_session_page_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<arrow_array::RecordBatch>, u64)> {
        validate_session_page_range(offset, limit, self.limits.max_rows)?;
        let plan = self.prepared_session_plan(sql, parameters).await?;
        let validation_plan = plan.clone();
        let frame = self
            .context
            .execute_logical_plan(plan)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_frame = frame
            .clone()
            .aggregate(
                Vec::new(),
                vec![datafusion::functions_aggregate::expr_fn::count(lit(1))
                    .alias("ugoite_session_count")],
            )
            .map_err(AuthorizedQueryError::execution_failed)?;
        let count_batches = self.collect_frame(count_frame).await?;
        let total = count_from_batches(&count_batches)?;
        let page = frame
            .limit(offset, Some(limit))
            .map_err(AuthorizedQueryError::resource_limit)?;
        let batches = self.collect_frame(page).await?;
        self.validate_revision_invariants(&validation_plan).await?;
        Ok((batches, total))
    }

    async fn execute_session_count_with_permit(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<u64> {
        let plan = self.prepared_session_plan(sql, parameters).await?;
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
                    .alias("ugoite_session_count")],
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

    async fn prepared_session_plan(
        &self,
        sql: &str,
        parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    ) -> Result<LogicalPlan> {
        let plan = self.bound_logical_plan(sql, parameters).await?;
        validate_sql_session_logical_plan(&plan, &self.authorized_relations)
            .map_err(AuthorizedQueryError::invalid_query)?;
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
        let plan = self
            .context
            .state()
            .create_logical_plan(sql)
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
        let batches = datafusion::physical_plan::collect(physical, task_context)
            .await
            .map_err(classify_datafusion_error)?;
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

fn logical_plan_contains_sort(plan: &LogicalPlan) -> bool {
    matches!(plan, LogicalPlan::Sort(_))
        || plan
            .inputs()
            .iter()
            .any(|input| logical_plan_contains_sort(input))
}

fn validate_session_page_range(offset: usize, limit: usize, max_rows: usize) -> Result<()> {
    let requested_rows = offset.checked_add(limit).ok_or_else(|| {
        AuthorizedQueryError::resource_limit(anyhow!("SQL session page range overflows"))
    })?;
    if limit == 0 || limit > max_rows || requested_rows > max_rows {
        return Err(AuthorizedQueryError::resource_limit(anyhow!(
            "SQL session page exceeds its configured row limit"
        ))
        .into());
    }
    Ok(())
}

/// Validates the session-only subset after DataFusion has resolved names and
/// aliases but before optimization. Generic authorized SQL intentionally
/// supports a broader read-only language; session paging does not.
fn validate_sql_session_logical_plan(
    plan: &LogicalPlan,
    authorized_relations: &BTreeSet<String>,
) -> Result<()> {
    let mut sort_count = 0;
    validate_sql_session_logical_plan_node(plan, authorized_relations, &mut sort_count)?;
    if sort_count != 1 {
        bail!("SQL session paging requires exactly one explicit ORDER BY");
    }
    Ok(())
}

fn validate_sql_session_logical_plan_node(
    plan: &LogicalPlan,
    authorized_relations: &BTreeSet<String>,
    sort_count: &mut usize,
) -> Result<()> {
    match plan {
        LogicalPlan::Sort(sort) => {
            *sort_count += 1;
            let last = sort.expr.last().ok_or_else(|| {
                anyhow!("SQL session paging requires ORDER BY ending with _ugoite_id")
            })?;
            if !sort_expression_resolves_to_external_id(&last.expr, &sort.input) {
                bail!("SQL session paging requires ORDER BY ending with the Form external ID");
            }
        }
        LogicalPlan::Subquery(_)
        | LogicalPlan::Join(_)
        | LogicalPlan::Aggregate(_)
        | LogicalPlan::Distinct(_)
        | LogicalPlan::Union(_)
        | LogicalPlan::Window(_)
        | LogicalPlan::Unnest(_)
        | LogicalPlan::Values(_)
        | LogicalPlan::EmptyRelation(_) => {
            bail!("SQL session paging supports only a simple single-Form SELECT")
        }
        LogicalPlan::TableScan(scan) => {
            let relation = scan.table_name.to_string();
            if !authorized_relations.contains(&relation) {
                bail!("query plan scans an unauthorized relation {relation}");
            }
        }
        LogicalPlan::SubqueryAlias(alias)
            if authorized_relations.contains(&alias.alias.to_string()) =>
        {
            // A public relation expands to Ugoite's trusted latest-revision
            // view, which contains an internal aggregate and join. Those
            // operators are not user SQL and must remain outside this
            // session-subset check.
            return Ok(());
        }
        LogicalPlan::Projection(_)
        | LogicalPlan::Filter(_)
        | LogicalPlan::Repartition(_)
        | LogicalPlan::SubqueryAlias(_)
        | LogicalPlan::Limit(_) => {}
        LogicalPlan::Explain(_)
        | LogicalPlan::Analyze(_)
        | LogicalPlan::Dml(_)
        | LogicalPlan::Ddl(_)
        | LogicalPlan::Copy(_)
        | LogicalPlan::Statement(_)
        | LogicalPlan::DescribeTable(_)
        | LogicalPlan::Extension(_)
        | LogicalPlan::RecursiveQuery(_) => {
            bail!("SQL session paging supports only a SELECT statement")
        }
    }
    for input in plan.inputs() {
        validate_sql_session_logical_plan_node(input, authorized_relations, sort_count)?;
    }
    Ok(())
}

/// Follows the resolved expression through projections, filters, aliases, and
/// scans. This rejects an output alias called `_ugoite_id` unless its lineage
/// reaches the provider's real external-ID column.
fn sort_expression_resolves_to_external_id(expr: &Expr, input: &LogicalPlan) -> bool {
    match input {
        LogicalPlan::Projection(projection) => {
            let Expr::Column(column) = expr else {
                return false;
            };
            let mut candidates = projection.expr.iter().filter(|candidate| {
                expression_output_name(candidate) == Some(column.name.as_str())
            });
            let Some(candidate) = candidates.next() else {
                return false;
            };
            if candidates.next().is_some() {
                return false;
            }
            if expression_is_external_id_source(candidate) {
                return true;
            }
            sort_expression_resolves_to_external_id(candidate, &projection.input)
        }
        LogicalPlan::Filter(filter) => sort_expression_resolves_to_external_id(expr, &filter.input),
        LogicalPlan::Sort(sort) => sort_expression_resolves_to_external_id(expr, &sort.input),
        LogicalPlan::Repartition(repartition) => {
            sort_expression_resolves_to_external_id(expr, &repartition.input)
        }
        LogicalPlan::Limit(limit) => sort_expression_resolves_to_external_id(expr, &limit.input),
        LogicalPlan::SubqueryAlias(alias) => {
            sort_expression_resolves_to_external_id(expr, &alias.input)
        }
        LogicalPlan::TableScan(scan) => matches!(expr, Expr::Column(column)
            if column.name.eq_ignore_ascii_case("ugoite_entry_external_id")
                || (column.name.eq_ignore_ascii_case("_ugoite_id")
                    && !scan.table_name.to_string().starts_with(INTERNAL_RELATION_PREFIX))),
        _ => false,
    }
}

fn expression_is_external_id_source(expr: &Expr) -> bool {
    match expr {
        Expr::Column(column) => column.name.eq_ignore_ascii_case("ugoite_entry_external_id"),
        Expr::Alias(alias) => matches!(
            alias.expr.as_ref(),
            Expr::Column(column) if column.name.eq_ignore_ascii_case("ugoite_entry_external_id")
        ),
        _ => false,
    }
}

fn expression_output_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Alias(alias) => Some(alias.name.as_str()),
        Expr::Column(column) => Some(column.name.as_str()),
        _ => None,
    }
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

fn system_column(column: &QuerySystemColumn) -> VisibleColumn {
    let (source, name) = match column {
        QuerySystemColumn::ExternalId => ("ugoite_entry_external_id", "_ugoite_id"),
        QuerySystemColumn::Title => ("ugoite_entry_title", "_ugoite_title"),
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
        LogicalPlan::Explain(_) | LogicalPlan::Analyze(_) => bail!("EXPLAIN is not supported"),
        LogicalPlan::Dml(_)
        | LogicalPlan::Ddl(_)
        | LogicalPlan::Copy(_)
        | LogicalPlan::Statement(_)
        | LogicalPlan::DescribeTable(_)
        | LogicalPlan::Extension(_)
        | LogicalPlan::RecursiveQuery(_) => bail!("statement kind is not supported"),
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
