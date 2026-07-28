//! Closed DataFusion context for authorized Iceberg queries.
//!
//! The public type deliberately exposes only `execute`. It never returns a
//! `SessionContext`, Catalog, provider, or SQL planner that could resolve an
//! unapproved object.

use anyhow::{anyhow, bail, Context, Result};
use datafusion::execution::context::SessionContext;
use datafusion::execution::memory_pool::GreedyMemoryPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::execution::{SessionStateBuilder, SessionStateDefaults};
use datafusion::logical_expr::expr_fn::ident;
use datafusion::logical_expr::LogicalPlan;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::{col, lit, SessionConfig};
use iceberg_datafusion::IcebergStaticTableProvider;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Semaphore;
use ugoite_core::query::{AuthorizedQueryPolicy, EntryScope, QuerySystemColumn};

use crate::{form_from_table, IcebergWorkspace};

const INTERNAL_RELATION_PREFIX: &str = "__ugoite_authorized_source_";

/// A query surface containing only Core-authorized, read-only logical Form
/// views. The underlying context remains private so callers cannot register a
/// table, UDF, object store, or provider of their own.
pub struct AuthorizedQueryContext {
    context: SessionContext,
    limits: ugoite_core::query::QueryLimits,
    permits: Arc<Semaphore>,
    authorized_relations: BTreeSet<String>,
    authorized_scans: BTreeSet<AuthorizedScan>,
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

impl std::fmt::Display for AuthorizedQueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidQuery { .. } => "invalid authorized query",
            Self::UnauthorizedQueryFeature { .. } => "unauthorized query feature",
            Self::ResourceLimitExceeded { .. } => "authorized query resource limit exceeded",
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
        self.authorized_query_context_inner(policy)
            .await
            .map_err(|error| {
                if error.to_string().contains("Resources exhausted") {
                    AuthorizedQueryError::resource_limit(error).into()
                } else {
                    AuthorizedQueryError::execution_failed(error).into()
                }
            })
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
            let provider = match snapshot_id {
                Some(snapshot_id) => {
                    IcebergStaticTableProvider::try_new_from_table_snapshot(table, snapshot_id)
                        .await
                        .context("open checkpoint-pinned Iceberg provider")?
                }
                None => IcebergStaticTableProvider::try_new_from_table(table)
                    .await
                    .context("open static Iceberg provider")?,
            };

            authorized_scans.insert(authorized_scan);

            let internal = format!("{INTERNAL_RELATION_PREFIX}{}", form_id.as_uuid().simple());
            relations.insert(internal.clone());
            context.register_table(internal.as_str(), Arc::new(provider))?;
            let visible = visible_columns(&form, form_policy)?;
            let source = context.table(internal.as_str()).await?;
            let scoped = match &form_policy.entry_scope {
                EntryScope::AllCurrent => source,
                EntryScope::Only(entry_ids) if entry_ids.is_empty() => source.filter(lit(false))?,
                EntryScope::Only(entry_ids) => source.filter(
                    col("entry_id").in_list(
                        entry_ids
                            .iter()
                            .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                            .collect::<Vec<_>>(),
                        false,
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
            let duplicates = heads
                .clone()
                .aggregate(
                    vec![col("entry_id")],
                    vec![
                        datafusion::functions_aggregate::expr_fn::count(lit(1)).alias("head_count")
                    ],
                )?
                .filter(col("head_count").not_eq(lit(1)))?
                .collect()
                .await?;
            if duplicates.iter().any(|batch| batch.num_rows() > 0) {
                bail!(
                    "entry revision invariant failed: multiple revisions share a maximum entry_version"
                );
            }
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

        let max_concurrency = policy.limits.max_concurrency;
        Ok(AuthorizedQueryContext {
            context,
            limits: policy.limits,
            permits: Arc::new(Semaphore::new(max_concurrency)),
            authorized_relations: relations,
            authorized_scans,
        })
    }
}

impl AuthorizedQueryContext {
    /// Plans and executes a read-only statement. User predicates are evaluated
    /// above the trusted Entry filter embedded in every registered view.
    pub async fn execute(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(AuthorizedQueryError::resource_limit)?;
        tokio::time::timeout(self.limits.timeout, self.execute_with_permit(sql))
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

    async fn execute_with_permit(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        let plan = self
            .context
            .state()
            .create_logical_plan(sql)
            .await
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(&plan, &self.authorized_relations)
            .map_err(AuthorizedQueryError::unauthorized)?;
        let optimized = self
            .context
            .state()
            .optimize(&plan)
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(&optimized, &self.authorized_relations)
            .map_err(AuthorizedQueryError::unauthorized)?;
        let frame = self
            .context
            .execute_logical_plan(optimized)
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
        let task_context = Arc::new(frame.task_ctx());
        let physical = frame
            .create_physical_plan()
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        validate_physical_plan(&physical, &self.authorized_scans)
            .map_err(AuthorizedQueryError::unauthorized)?;
        let batches = datafusion::physical_plan::collect(physical, task_context)
            .await
            .map_err(AuthorizedQueryError::execution_failed)?;
        let rows = batches.iter().map(|batch| batch.num_rows()).sum::<usize>();
        if rows > self.limits.max_rows {
            return Err(AuthorizedQueryError::resource_limit(anyhow!(
                "authorized query row limit exceeded"
            ))
            .into());
        }
        Ok(batches)
    }
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
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(column) = policy
        .columns
        .iter()
        .find(|column| !form_columns.contains(column.as_str()))
    {
        bail!("authorized query policy exposes unknown Form column {column}");
    }
    let mut visible = policy
        .columns
        .iter()
        .map(|column| VisibleColumn {
            // Form field names are immutable Iceberg column names.
            source: column.clone(),
            // Unquoted DataFusion identifiers are lowercase. Preserve the
            // physical Iceberg name internally while exposing a stable,
            // case-insensitive SQL surface.
            name: column.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>();
    if let Some(column) = policy
        .system_columns
        .iter()
        .map(|column| column.as_str())
        .find(|column| form_columns.contains(column))
    {
        bail!("Form column {column} collides with a query system column");
    }
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
        QuerySystemColumn::ExternalId => ("ugoite_entry_external_id", "id"),
        QuerySystemColumn::Title => ("ugoite_entry_title", "title"),
        QuerySystemColumn::CreatedAt => ("ugoite_entry_created_at", "created_at"),
        QuerySystemColumn::UpdatedAt => ("ugoite_entry_updated_at", "updated_at"),
        QuerySystemColumn::EntryId => ("entry_id", "entry_id"),
        QuerySystemColumn::EntryVersion => ("entry_version", "entry_version"),
        QuerySystemColumn::CommittedAt => ("committed_at", "committed_at"),
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
