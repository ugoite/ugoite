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
use std::collections::{BTreeMap, BTreeSet};
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
    trusted_relations: BTreeMap<String, TrustedRelation>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AuthorizedScan {
    table_uuid: String,
    snapshot_id: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrustedRelation {
    relation: String,
    readable_entry_ids: BTreeSet<Vec<u8>>,
    visible_columns: BTreeSet<String>,
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
            .map_err(|error| AuthorizedQueryError::execution_failed(error).into())
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
        let context = SessionContext::new_with_config_rt(config, runtime);
        let mut relations = BTreeSet::new();
        let mut authorized_scans = BTreeSet::new();
        let mut trusted_relations = BTreeMap::new();

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
            trusted_relations.insert(
                internal.clone(),
                TrustedRelation {
                    relation: form_policy.relation.clone(),
                    readable_entry_ids: form_policy
                        .readable_entry_ids
                        .iter()
                        .map(|entry_id| entry_id.as_uuid().as_bytes().to_vec())
                        .collect(),
                    visible_columns: visible.iter().cloned().collect(),
                },
            );
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
            trusted_relations,
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

    async fn execute_with_permit(&self, sql: &str) -> Result<Vec<arrow_array::RecordBatch>> {
        let plan = self
            .context
            .state()
            .create_logical_plan(sql)
            .await
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(
            &plan,
            &self.limits.allowed_functions,
            &self.authorized_relations,
        )
        .map_err(AuthorizedQueryError::unauthorized)?;
        let optimized = self
            .context
            .state()
            .optimize(&plan)
            .map_err(AuthorizedQueryError::invalid_query)?;
        validate_logical_plan(
            &optimized,
            &self.limits.allowed_functions,
            &self.authorized_relations,
        )
        .map_err(AuthorizedQueryError::unauthorized)?;
        validate_trusted_entry_filters(&optimized, &self.trusted_relations)
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
            validate_expression_function(expression, allowed_functions)?;
            Ok(TreeNodeRecursion::Continue)
        })?;
    }
    for input in plan.inputs() {
        validate_logical_plan(input, allowed_functions, authorized_relations)?;
    }
    Ok(())
}

/// Keep this match exhaustive. DataFusion represents function calls in several
/// expression variants, and a future variant must fail compilation here until
/// its authorization semantics are deliberately reviewed.
#[allow(deprecated)]
fn validate_expression_function(
    expression: &Expr,
    allowed_functions: &BTreeSet<String>,
) -> datafusion::error::Result<()> {
    match expression {
        Expr::ScalarFunction(function) => authorize_function(function.name(), allowed_functions),
        Expr::AggregateFunction(function) => {
            authorize_function(function.func.name(), allowed_functions)
        }
        Expr::WindowFunction(function) => {
            authorize_function(function.fun.name(), allowed_functions)
        }
        // DataFusion's UNNEST is a distinct expression/plan form rather than
        // a ScalarFunction. It remains unavailable unless Core admits it.
        Expr::Unnest(_) => authorize_function("unnest", allowed_functions),
        Expr::Alias(_)
        | Expr::Column(_)
        | Expr::ScalarVariable(_, _)
        | Expr::Literal(_, _)
        | Expr::BinaryExpr(_)
        | Expr::Like(_)
        | Expr::SimilarTo(_)
        | Expr::Not(_)
        | Expr::IsNotNull(_)
        | Expr::IsNull(_)
        | Expr::IsTrue(_)
        | Expr::IsFalse(_)
        | Expr::IsUnknown(_)
        | Expr::IsNotTrue(_)
        | Expr::IsNotFalse(_)
        | Expr::IsNotUnknown(_)
        | Expr::Negative(_)
        | Expr::Between(_)
        | Expr::Case(_)
        | Expr::Cast(_)
        | Expr::TryCast(_)
        | Expr::InList(_)
        | Expr::Exists(_)
        | Expr::InSubquery(_)
        | Expr::SetComparison(_)
        | Expr::ScalarSubquery(_)
        | Expr::Wildcard { .. }
        | Expr::GroupingSet(_)
        | Expr::Placeholder(_)
        | Expr::OuterReferenceColumn(_, _) => Ok(()),
    }
}

fn authorize_function(
    name: &str,
    allowed_functions: &BTreeSet<String>,
) -> datafusion::error::Result<()> {
    if allowed_functions.contains(&name.to_ascii_lowercase()) {
        Ok(())
    } else {
        Err(datafusion::error::DataFusionError::Plan(format!(
            "function {name} is not authorized"
        )))
    }
}

/// Verifies that every internal Iceberg relation remains guarded by the Entry
/// boundary installed while building the trusted view. A user predicate can
/// only add another filter; it cannot make this filter disappear. This runs on
/// the optimized plan because predicate pushdown/rewrite happens after the
/// initial authorization check.
fn validate_trusted_entry_filters(
    plan: &LogicalPlan,
    trusted_relations: &BTreeMap<String, TrustedRelation>,
) -> Result<()> {
    let mut seen_scans = BTreeSet::new();
    let mut guarded_scans = BTreeSet::new();
    let mut projected_relations = BTreeSet::new();
    collect_trusted_entry_filters(
        plan,
        trusted_relations,
        &mut seen_scans,
        &mut guarded_scans,
        &mut projected_relations,
    )?;
    for internal in trusted_relations.keys() {
        if seen_scans.contains(internal) && !guarded_scans.contains(internal) {
            bail!("optimized query plan lost the trusted Entry authorization filter");
        }
        if seen_scans.contains(internal) && !projected_relations.contains(internal) {
            bail!("optimized query plan lost the trusted visible-column projection");
        }
    }
    Ok(())
}

fn collect_trusted_entry_filters(
    plan: &LogicalPlan,
    trusted_relations: &BTreeMap<String, TrustedRelation>,
    seen_scans: &mut BTreeSet<String>,
    guarded_scans: &mut BTreeSet<String>,
    projected_relations: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut descendants = BTreeSet::new();
    for input in plan.inputs() {
        descendants.extend(collect_trusted_entry_filters(
            input,
            trusted_relations,
            seen_scans,
            guarded_scans,
            projected_relations,
        )?);
    }
    if let LogicalPlan::TableScan(scan) = plan {
        let relation = scan.table_name.to_string();
        if let Some(trusted) = trusted_relations.get(&relation) {
            seen_scans.insert(relation.clone());
            descendants.insert(relation.clone());
            if scan
                .filters
                .iter()
                .any(|filter| is_trusted_entry_filter(filter, trusted))
            {
                guarded_scans.insert(relation);
            }
        }
    }
    if let LogicalPlan::Filter(filter) = plan {
        if descendants.len() == 1 {
            let internal = descendants.iter().next().expect("one relation");
            if let Some(trusted) = trusted_relations.get(internal) {
                if is_trusted_entry_filter(&filter.predicate, trusted) {
                    guarded_scans.insert(internal.clone());
                }
            }
        }
    }
    if let LogicalPlan::SubqueryAlias(alias) = plan {
        let visible = alias
            .schema
            .fields()
            .iter()
            .map(|field| field.name().to_string())
            .collect::<BTreeSet<_>>();
        for (internal, trusted) in trusted_relations {
            if alias.alias.to_string() == trusted.relation
                && visible.is_subset(&trusted.visible_columns)
            {
                projected_relations.insert(internal.clone());
            }
        }
    }
    Ok(descendants)
}

fn is_trusted_entry_filter(expression: &Expr, trusted: &TrustedRelation) -> bool {
    let mut entry_ids = BTreeSet::new();
    let _ = expression.apply(|candidate| {
        if let Expr::InList(list) = candidate {
            if !list.negated && is_entry_id_column(&list.expr) {
                entry_ids.extend(list.list.iter().filter_map(literal_binary));
            }
        }
        if let Expr::BinaryExpr(binary) = candidate {
            if binary.op == datafusion::logical_expr::Operator::Eq {
                if is_entry_id_column(&binary.left) {
                    if let Some(value) = literal_binary(&binary.right) {
                        entry_ids.insert(value);
                    }
                } else if is_entry_id_column(&binary.right) {
                    if let Some(value) = literal_binary(&binary.left) {
                        entry_ids.insert(value);
                    }
                }
            }
        }
        Ok(TreeNodeRecursion::Continue)
    });
    entry_ids == trusted.readable_entry_ids
}

fn is_entry_id_column(expression: &Expr) -> bool {
    matches!(expression, Expr::Column(column) if column.name == "entry_id")
}

fn literal_binary(expression: &Expr) -> Option<Vec<u8>> {
    match expression {
        Expr::Literal(datafusion::scalar::ScalarValue::Binary(Some(value)), _) => {
            Some(value.clone())
        }
        Expr::Literal(datafusion::scalar::ScalarValue::FixedSizeBinary(_, Some(value)), _) => {
            Some(value.clone())
        }
        _ => None,
    }
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
