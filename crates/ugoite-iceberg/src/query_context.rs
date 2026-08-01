//! Closed DataFusion context for authorized Iceberg queries.
//!
//! The public type deliberately exposes only `execute`. It never returns a
//! `SessionContext`, Catalog, provider, or SQL planner that could resolve an
//! unapproved object.

use anyhow::{anyhow, bail, Context, Result};
use datafusion::catalog::default_table_source::DefaultTableSource;
use datafusion::datasource::TableProvider;
use datafusion::execution::context::SessionContext;
use datafusion::execution::memory_pool::GreedyMemoryPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::execution::{SessionStateBuilder, SessionStateDefaults};
use datafusion::logical_expr::expr_fn::ident;
use datafusion::logical_expr::{Expr, LogicalPlan};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::{col, lit, DataFrame, SessionConfig};
use iceberg_datafusion::IcebergStaticTableProvider;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::Semaphore;
use ugoite_core::query::{AuthorizedQueryPolicy, EntryScope, QuerySystemColumn};
use ugoite_domain::form::sql_column_name;

use crate::{form_from_table, IcebergWorkspace};

const INTERNAL_RELATION_PREFIX: &str = "__ugoite_authorized_source_";

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
        policy
            .limits
            .validate()
            .map_err(|message| anyhow!(message))?;

        let config = SessionConfig::new()
            .with_information_schema(false)
            .with_target_partitions(policy.limits.max_concurrency);
        let runtime = RuntimeEnvBuilder::new()
            .with_memory_pool(Arc::new(GreedyMemoryPool::new(
                policy.limits.max_memory_bytes,
            )))
            .build_arc()
            .context("configure bounded DataFusion runtime")?;
        // Start from an empty SessionState. Registering only Core-approved
        // built-ins makes every other scalar, aggregate, window, and table
        // function unresolvable before plan validation. The empty default
        // catalog is retained solely for relation registration; no file
        // formats, table factories, function factory, or table functions are
        // installed.
        let allowed_functions = &policy.limits.allowed_functions;
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
        let context = SessionContext::new_with_state(state);
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
            let (form, table, snapshot_id) = match &policy.checkpoint {
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
            let authorized_scan = AuthorizedScan {
                table_uuid: table.metadata().uuid().to_string(),
                snapshot_id,
            };
            let provider: Arc<dyn TableProvider> = Arc::new(match snapshot_id {
                Some(snapshot_id) => {
                    IcebergStaticTableProvider::try_new_from_table_snapshot(table, snapshot_id)
                        .await
                        .context("open checkpoint-pinned Iceberg provider")?
                }
                None => IcebergStaticTableProvider::try_new_from_table(table)
                    .await
                    .context("open static Iceberg provider")?,
            });

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
        })
    }
}

impl AuthorizedQueryContext {
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
        for (provider, check) in &self.duplicate_head_checks {
            let provider_address = Arc::as_ptr(provider) as *const () as usize;
            if !scanned_providers.contains(&provider_address) {
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
        }
        Ok(())
    }
}

fn collect_scanned_provider_addresses(plan: &LogicalPlan, providers: &mut BTreeSet<usize>) {
    if let LogicalPlan::TableScan(scan) = plan {
        if let Some(source) = scan.source.as_any().downcast_ref::<DefaultTableSource>() {
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
        QuerySystemColumn::CreatedAt => ("ugoite_entry_created_at", "_ugoite_created_at"),
        QuerySystemColumn::UpdatedAt => ("ugoite_entry_updated_at", "_ugoite_updated_at"),
        QuerySystemColumn::EntryId => ("entry_id", "_ugoite_entry_id"),
        QuerySystemColumn::EntryVersion => ("entry_version", "_ugoite_entry_version"),
        QuerySystemColumn::CommittedAt => ("committed_at", "_ugoite_committed_at"),
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
    if let Some(scan) = plan
        .as_any()
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
