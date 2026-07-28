//! Closed DataFusion context for authorized Iceberg queries.
//!
//! The public type deliberately exposes only `execute`. It never returns a
//! `SessionContext`, Catalog, provider, or SQL planner that could resolve an
//! unapproved object.

use anyhow::{anyhow, bail, Context, Result};
use datafusion::common::tree_node::{TreeNode, TreeNodeRecursion};
use datafusion::execution::context::SessionContext;
use datafusion::execution::memory_pool::GreedyMemoryPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::logical_expr::{Expr, LogicalPlan};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::{col, lit, SessionConfig};
use iceberg_datafusion::IcebergStaticTableProvider;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Semaphore;
use ugoite_core::query::AuthorizedQueryPolicy;

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

impl IcebergWorkspace {
    /// Translates a Core authorization decision into a closed DataFusion query
    /// surface. All providers are static Iceberg providers; a requested
    /// checkpoint requires one snapshot for every exposed Form.
    pub async fn authorized_query_context(
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
        let context = SessionContext::new_with_config_rt(config, runtime);
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
            let entry_ids = form_policy
                .readable_entry_ids
                .iter()
                .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                .collect::<Vec<_>>();
            let visible_refs = visible.iter().map(String::as_str).collect::<Vec<_>>();
            let source = context.table(internal.as_str()).await?;
            let filtered = if entry_ids.is_empty() {
                source.filter(lit(false))?
            } else {
                source.filter(col("entry_id").in_list(entry_ids, false))?
            }
            .select_columns(&visible_refs)?;
            let view = filtered.into_view();
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
            .map_err(|_| anyhow!("authorized query concurrency limit reached"))?;
        tokio::time::timeout(self.limits.timeout, self.execute_with_permit(sql))
            .await
            .map_err(|_| anyhow!("authorized query timed out"))?
    }

    async fn execute_with_permit(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        let plan = self
            .context
            .state()
            .create_logical_plan(sql)
            .await
            .context("plan authorized query")?;
        validate_logical_plan(
            &plan,
            &self.limits.allowed_functions,
            &self.authorized_relations,
        )?;
        let optimized = self.context.state().optimize(&plan)?;
        validate_logical_plan(
            &optimized,
            &self.limits.allowed_functions,
            &self.authorized_relations,
        )?;
        let frame = self.context.execute_logical_plan(optimized).await?;
        let max_rows_with_sentinel = self
            .limits
            .max_rows
            .checked_add(1)
            .context("authorized query row limit is too large")?;
        let frame = frame.limit(0, Some(max_rows_with_sentinel))?;
        let task_context = Arc::new(frame.task_ctx());
        let physical = frame.create_physical_plan().await?;
        validate_physical_plan(&physical, &self.authorized_scans)?;
        let batches = datafusion::physical_plan::collect(physical, task_context).await?;
        let rows = batches.iter().map(|batch| batch.num_rows()).sum::<usize>();
        if rows > self.limits.max_rows {
            bail!("authorized query row limit exceeded");
        }
        Ok(batches)
    }
}

fn visible_columns(
    form: &ugoite_domain::form::FormDefinition,
    policy: &ugoite_core::query::AuthorizedQueryForm,
) -> Result<Vec<String>> {
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
    let mut visible = policy.columns.iter().cloned().collect::<Vec<_>>();
    if let Some(column) = policy
        .system_columns
        .iter()
        .map(|column| column.as_str())
        .find(|column| form_columns.contains(column))
    {
        bail!("Form column {column} collides with a query system column");
    }
    visible.extend(
        policy
            .system_columns
            .iter()
            .map(|column| column.as_str().to_string()),
    );
    if visible.is_empty() {
        bail!(
            "authorized query policy exposes no columns for {}",
            policy.relation
        );
    }
    Ok(visible)
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
    allowed_functions: &BTreeSet<String>,
    authorized_relations: &BTreeSet<String>,
) -> Result<()> {
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
    for expression in plan.expressions() {
        expression.apply(|expression| {
            let name = match expression {
                Expr::ScalarFunction(function) => Some(function.name()),
                Expr::AggregateFunction(function) => Some(function.func.name()),
                Expr::WindowFunction(function) => Some(function.fun.name()),
                _ => None,
            };
            if let Some(name) = name {
                if !allowed_functions.contains(&name.to_ascii_lowercase()) {
                    return Err(datafusion::error::DataFusionError::Plan(format!(
                        "function {name} is not authorized"
                    )));
                }
            }
            Ok(TreeNodeRecursion::Continue)
        })?;
    }
    for input in plan.inputs() {
        validate_logical_plan(input, allowed_functions, authorized_relations)?;
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
        // The Entry predicate is owned by the Core-built view and may remain
        // in a FilterExec rather than be pushed into IcebergTableScan. The
        // optimized logical-plan validation above proves that this scan is
        // reachable only through an authorized relation/view.
    } else if !is_authorized_physical_node(plan.name()) {
        bail!("physical plan node {} is not authorized", plan.name());
    }
    for child in plan.children() {
        validate_physical_plan(child, authorized_scans)?;
    }
    Ok(())
}

fn is_authorized_physical_node(name: &str) -> bool {
    matches!(
        name,
        "AggregateExec"
            | "CoalesceBatchesExec"
            | "CoalescePartitionsExec"
            | "CooperativeExec"
            | "CrossJoinExec"
            | "EmptyExec"
            | "FilterExec"
            | "GlobalLimitExec"
            | "HashJoinExec"
            | "LocalLimitExec"
            | "NestedLoopJoinExec"
            | "ProjectionExec"
            | "RepartitionExec"
            | "SortExec"
            | "SortMergeJoinExec"
            | "SortPreservingMergeExec"
            | "UnionExec"
            | "WindowAggExec"
            | "BoundedWindowAggExec"
            | "UnnestExec"
    )
}
