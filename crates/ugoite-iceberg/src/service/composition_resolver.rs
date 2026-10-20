//! Storage-backed Composition resolution shared by local and server clients.
//!
//! The service reads one exact Composition revision and derives current query
//! descriptors, then delegates binding and query compilation to
//! `ugoite_core::composition`. It does not execute queries or persist runtime
//! state.

use anyhow::{anyhow, Result};
use datafusion::scalar::ScalarValue;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use ugoite_core::composition::{
    bind_variables_with_unset, resolve_composition_authoring_preview_with_query_selection,
    resolve_composition_preview_with_query_selection, resolve_composition_with_query_selection,
    returns_from_schema, CompositionDiagnostic, CompositionRevisionRef, CurrentQueryDescriptor,
    InlineSqlQueryMetadata, PreviewCompositionPlan, ResolveInput, ResolvedCompositionPlan,
    SavedSqlRevisionMetadata,
};
use ugoite_core::composition_query::selected_query_dependency_closure_for_graph;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
    SqlFunctionPolicy,
};
use ugoite_core::sql_query::{
    SavedSqlRevisionRef, SqlResultColumnType, MAX_SQL_COLUMN_NAME_BYTES, MAX_SQL_OUTPUT_COLUMNS,
};
use ugoite_domain::composition::{
    canonicalize_composition, parse_composition_yaml, variable_graph_dependency_order,
    CompositionDiagnosticCode, CompositionDocument, VariableGraph, VariableGraphColumn,
    VariableGraphFormBinding, VariableGraphNode, VariableGraphNodeKind,
    VariableGraphQueryDefinition, VariableGraphResultType, VariableGraphScalarDescriptor,
    VariableGraphScalarType,
};
use ugoite_domain::form::{sql_column_name, sql_relation_name, FormDefinition};
use ugoite_domain::id::{validate_revision_id, validate_sql_id, FormId};
use uuid::Uuid;

use super::{
    validate_storage_id, UgoiteService, MAX_AUTHORIZED_SCOPE_FORMS,
    MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES,
};
use crate::integrity::RealIntegrityProvider;
use crate::{iceberg_store, saved_sql};

/// The semantic outcome of resolving one exact Composition revision.
///
/// A missing plan carries stable resolver diagnostics. Storage, identity, and
/// authorization failures remain `Err`, matching the existing service read
/// boundary. Variable definitions are present once the YAML envelope parses,
/// including when supplied variable values are invalid.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionResolution {
    pub variable_definitions: Option<Vec<VariableGraphNode>>,
    pub plan: Option<ResolvedCompositionPlan>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

/// The semantic outcome of previewing one unsaved candidate document.
///
/// Identity is the draft fingerprint: there is no entry or revision
/// reference, and the plan must never be stored, published, or given
/// history. Storage, identity, and authorization failures remain `Err`,
/// matching the existing service read boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionPreview {
    pub draft_fingerprint: Option<String>,
    pub variable_definitions: Option<Vec<VariableGraphNode>>,
    pub plan: Option<PreviewCompositionPlan>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

enum OwnedCurrentQueryDescriptor {
    InlineSql {
        variable_id: String,
        schema: Option<InlineSqlQueryMetadata>,
    },
    SavedSql {
        variable_id: String,
        revision: Option<SavedSqlRevisionMetadata>,
    },
}

/// Return a deterministic order that derives every upstream query result
/// contract before planning a query that binds to it. Invalid drafts retain
/// their original order and will be rejected by the canonical graph resolver.
pub(crate) fn variable_graph_topological_order(
    variables: &[VariableGraphNode],
) -> Result<Vec<String>, Vec<ugoite_domain::composition::VariableGraphDiagnostic>> {
    variable_graph_dependency_order(&VariableGraph {
        variables: variables.to_vec(),
    })
}

fn query_selection_for_resolution(
    variables: &[VariableGraphNode],
    requested_queries: Option<&[String]>,
) -> Option<BTreeSet<String>> {
    requested_queries.map(|requested_queries| {
        let graph = VariableGraph {
            variables: variables.to_vec(),
        };
        // Selection validation is reported by the evaluator after the plan is
        // assembled. An invalid selection therefore derives no SQL metadata.
        selected_query_dependency_closure_for_graph(&graph, Some(requested_queries))
            .unwrap_or_default()
            .into_iter()
            .collect()
    })
}

impl OwnedCurrentQueryDescriptor {
    fn as_current_query(&self) -> CurrentQueryDescriptor<'_> {
        match self {
            Self::InlineSql {
                variable_id,
                schema,
            } => CurrentQueryDescriptor::InlineSql {
                variable_id,
                metadata: schema.as_ref(),
            },
            Self::SavedSql {
                variable_id,
                revision,
            } => CurrentQueryDescriptor::SavedSql {
                variable_id,
                metadata: revision.as_ref(),
            },
        }
    }
}

impl UgoiteService {
    async fn get_composition_source_form_local(
        &self,
        space_id: &str,
        form_id: FormId,
    ) -> Result<Option<ugoite_domain::form::FormDefinition>> {
        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_workspace_read_only(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        Ok(workspace
            .list_forms_bounded(
                MAX_AUTHORIZED_SCOPE_FORMS,
                MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES,
            )
            .await?
            .into_iter()
            .find(|form| form.id == form_id))
    }

    async fn get_saved_sql_revision_descriptor_local(
        &self,
        space_id: &str,
        source: &SavedSqlRevisionRef,
    ) -> Result<Option<saved_sql::SavedSqlRevisionDescriptor>> {
        validate_storage_id(validate_sql_id(&source.id))?;
        validate_storage_id(validate_revision_id(&source.revision_id))?;
        self.validate_complete_space(space_id).await?;

        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let revision = match saved_sql::read_sql_revision(
            &self.operator,
            &self.workspace_path(space_id),
            &source.id,
            &source.revision_id,
            &integrity,
        )
        .await
        {
            Ok(revision) => revision,
            Err(error)
                if error.chain().any(|cause| {
                    cause
                        .downcast_ref::<AppError>()
                        .is_some_and(|app| app.code() == ErrorCode::EntryNotFound)
                }) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };

        let mut variables = BTreeMap::new();
        for variable in revision.variables.as_array().into_iter().flatten() {
            let name = variable
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Saved SQL variable name is missing"))?;
            let var_type = variable
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Saved SQL variable type is missing"))?;
            variables.insert(
                name.to_owned(),
                saved_sql::SavedSqlVariableDescriptor {
                    var_type: var_type.to_owned(),
                },
            );
        }
        Ok(Some(saved_sql::SavedSqlRevisionDescriptor {
            id: source.id.clone(),
            revision_id: source.revision_id.clone(),
            variables,
        }))
    }

    async fn read_saved_sql_revision_for_metadata(
        &self,
        space_id: &str,
        source: &SavedSqlRevisionRef,
    ) -> Result<Option<saved_sql::ResolvedSavedSqlRevision>> {
        validate_storage_id(validate_sql_id(&source.id))?;
        validate_storage_id(validate_revision_id(&source.revision_id))?;
        self.validate_complete_space(space_id).await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        match saved_sql::read_sql_revision(
            &self.operator,
            &self.workspace_path(space_id),
            &source.id,
            &source.revision_id,
            &integrity,
        )
        .await
        {
            Ok(revision) => Ok(Some(revision)),
            Err(error)
                if error.chain().any(|cause| {
                    cause
                        .downcast_ref::<AppError>()
                        .is_some_and(|app| app.code() == ErrorCode::EntryNotFound)
                }) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Derive trusted metadata for an inline SQL query variable.
    pub(crate) async fn derive_inline_sql_query_metadata(
        &self,
        space_id: &str,
        node: &VariableGraphNode,
        trusted_bindings: Option<&[VariableGraphFormBinding]>,
        variables: &[VariableGraphNode],
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<InlineSqlQueryMetadata>> {
        let VariableGraphNodeKind::Query { query, .. } = &node.kind else {
            return Err(anyhow!("expected a query variable"));
        };
        let VariableGraphQueryDefinition::InlineSql { sql } = &query.definition else {
            return Err(anyhow!("expected an inline SQL query variable"));
        };
        let parameter_types = query_parameter_types(query.arguments.as_slice(), variables)?;
        let planned = self
            .plan_sql_query_metadata(
                space_id,
                sql,
                trusted_bindings,
                &parameter_types,
                principal_ids,
                authorization_state,
            )
            .await?;
        Ok(planned.map(|planned| InlineSqlQueryMetadata {
            bound_sql: planned.bound_sql,
            form_bindings: planned.form_bindings,
            parameter_names: planned.parameter_names,
            result_columns: planned.result_columns,
            schema_fingerprint: planned.schema_fingerprint,
        }))
    }

    /// Derive the descriptor for the exact Saved SQL revision pinned by a query variable.
    pub(crate) async fn derive_saved_sql_query_metadata(
        &self,
        space_id: &str,
        node: &VariableGraphNode,
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<SavedSqlRevisionMetadata>> {
        let VariableGraphNodeKind::Query { query, returns } = &node.kind else {
            return Err(anyhow!("expected a query variable"));
        };
        let VariableGraphQueryDefinition::SavedSql {
            entry_id: Some(entry_id),
            revision_id: Some(revision_id),
            column,
        } = &query.definition
        else {
            return Ok(None);
        };
        let saved_sql = SavedSqlRevisionRef {
            id: entry_id.clone(),
            revision_id: revision_id.clone(),
        };
        let (descriptor, revision) = if let Some(state) = authorization_state {
            let principal_ids = principal_ids
                .ok_or_else(|| anyhow!("authorization state requires principal IDs"))?;
            validate_storage_id(validate_sql_id(&saved_sql.id))?;
            validate_storage_id(validate_revision_id(&saved_sql.revision_id))?;
            super::ensure_saved_sql_access(state, principal_ids, &saved_sql)?;
            self.validate_complete_space(space_id).await?;
            let Some(revision) = self
                .read_saved_sql_revision_for_metadata(space_id, &saved_sql)
                .await?
            else {
                return Ok(None);
            };
            let descriptor = saved_sql_revision_descriptor(&saved_sql, &revision)?;
            (descriptor, revision)
        } else {
            let descriptor = match principal_ids {
                Some(principal_ids) => conceal_sql_query_lookup(
                    self.get_saved_sql_revision_descriptor_authorized_for_principals(
                        space_id,
                        &saved_sql,
                        principal_ids,
                    )
                    .await,
                    ErrorCode::EntryNotFound,
                )?,
                None => {
                    self.get_saved_sql_revision_descriptor_local(space_id, &saved_sql)
                        .await?
                }
            };
            let Some(descriptor) = descriptor else {
                return Ok(None);
            };
            let Some(revision) = self
                .read_saved_sql_revision_for_metadata(space_id, &saved_sql)
                .await?
            else {
                return Ok(None);
            };
            (descriptor, revision)
        };
        let bindings = revision
            .bindings
            .iter()
            .map(|binding| {
                let form_id = Uuid::parse_str(&binding.form_id)
                    .map(FormId::from_uuid)
                    .map_err(|error| anyhow!("Saved SQL Form binding ID is invalid: {error}"))?;
                Ok(VariableGraphFormBinding {
                    relation: binding.name.clone(),
                    form_id,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let parameter_types = descriptor
            .variables
            .iter()
            .map(|(name, variable)| (name.clone(), variable.var_type.clone()))
            .collect::<BTreeMap<_, _>>();
        let planned = self
            .plan_sql_query_metadata(
                space_id,
                &revision.sql,
                Some(&bindings),
                &parameter_types,
                principal_ids,
                authorization_state,
            )
            .await?;
        let Some(planned) = planned else {
            return Ok(None);
        };
        let result_columns = if let Some(column) = column {
            let matches = planned
                .result_columns
                .iter()
                .filter(|candidate| candidate.name == *column)
                .cloned()
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [column] if matches!(returns, VariableGraphResultType::Scalar { .. }) => {
                    vec![column.clone()]
                }
                _ => return Ok(None),
            }
        } else {
            planned.result_columns
        };
        let parameter_types = parameter_types
            .iter()
            .map(|(name, type_name)| {
                let scalar_type =
                    variable_graph_scalar_type_for_sql(type_name).ok_or_else(|| {
                        anyhow!("Saved SQL parameter type is unsupported by the query binder")
                    })?;
                Ok((
                    name.clone(),
                    VariableGraphScalarDescriptor {
                        scalar_type,
                        nullable: true,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Some(SavedSqlRevisionMetadata {
            entry_id: descriptor.id,
            revision_id: descriptor.revision_id,
            sql_text: revision.sql,
            parameter_types,
            form_bindings: planned.form_bindings,
            result_columns,
            schema_fingerprint: planned.schema_fingerprint,
        }))
    }

    async fn plan_sql_query_metadata(
        &self,
        space_id: &str,
        sql: &str,
        trusted_bindings: Option<&[VariableGraphFormBinding]>,
        parameter_types: &BTreeMap<String, String>,
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<PlannedSqlQueryMetadata>> {
        self.validate_complete_space(space_id).await?;
        let normalized_sql = crate::index::normalize_sql_template(sql)?;
        crate::index::validate_read_only_sql(&normalized_sql)?;
        let references = crate::index::quoted_form_name_references(&normalized_sql)?;
        let relation_names = crate::index::quoted_form_relation_names(&normalized_sql)?;
        let stable_relations = relation_names
            .difference(&references)
            .cloned()
            .collect::<BTreeSet<_>>();
        let (available_forms, available_scopes) = self
            .composition_form_snapshot(space_id, principal_ids, authorization_state)
            .await?;

        let mut bindings = match trusted_bindings {
            Some(bindings) => {
                let mut bindings = bindings.to_vec();
                let mut binding_names = bindings
                    .iter()
                    .map(|binding| binding.relation.clone())
                    .collect::<BTreeSet<_>>();
                if binding_names.len() != bindings.len() {
                    return Ok(None);
                }
                for relation in &stable_relations {
                    if binding_names.contains(relation) {
                        continue;
                    }
                    let Some(form) = available_forms
                        .iter()
                        .find(|form| sql_relation_name(form.id) == *relation)
                    else {
                        return Ok(None);
                    };
                    bindings.push(VariableGraphFormBinding {
                        relation: relation.clone(),
                        form_id: form.id,
                    });
                    binding_names.insert(relation.clone());
                }
                if binding_names != relation_names {
                    return Ok(None);
                }
                bindings
            }
            None => {
                let mut resolved = Vec::with_capacity(relation_names.len());
                for relation in &relation_names {
                    let Some(form) = available_forms.iter().find(|form| {
                        form.name == *relation || sql_relation_name(form.id) == *relation
                    }) else {
                        return Ok(None);
                    };
                    resolved.push(VariableGraphFormBinding {
                        relation: relation.clone(),
                        form_id: form.id,
                    });
                }
                resolved
            }
        };
        bindings.sort_by(|left, right| left.relation.cmp(&right.relation));

        let mut selected_forms = Vec::with_capacity(bindings.len());
        let mut relation_scopes = BTreeMap::new();
        for binding in &bindings {
            let form = match (principal_ids, authorization_state) {
                (Some(_), Some(_)) => match available_forms
                    .iter()
                    .find(|form| form.id == binding.form_id)
                {
                    Some(form) => form.clone(),
                    None => return Ok(None),
                },
                (Some(principal_ids), None) => match conceal_sql_query_lookup(
                    self.get_composition_source_form_authorized_for_principals(
                        space_id,
                        binding.form_id,
                        principal_ids,
                    )
                    .await,
                    ErrorCode::FormNotFound,
                )? {
                    Some(form) => form,
                    None => return Ok(None),
                },
                (None, _) => match self
                    .get_composition_source_form_local(space_id, binding.form_id)
                    .await?
                {
                    Some(form) => form,
                    None => return Ok(None),
                },
            };
            let scope = match principal_ids {
                Some(_) => match available_scopes.get(&form.name.to_ascii_lowercase()) {
                    Some(scope) => scope.clone(),
                    None => return Ok(None),
                },
                None => EntryScope::AllCurrent,
            };
            relation_scopes.insert(sql_relation_name(form.id), scope);
            selected_forms.push(form);
        }

        let aliases = bindings
            .iter()
            .map(|binding| (binding.relation.clone(), sql_relation_name(binding.form_id)))
            .collect::<BTreeMap<_, _>>();
        let (bound_sql, used_relations) =
            crate::index::resolve_saved_sql_bindings(&normalized_sql, &aliases)?;
        if used_relations != relation_names {
            return Ok(None);
        }
        let parameter_names = crate::index::datafusion_parameter_names(
            &self.operator,
            &self.workspace_path(space_id),
            &bound_sql,
        )
        .await?
        .into_iter()
        .map(|name| name.trim_start_matches('$').to_owned())
        .collect::<BTreeSet<_>>();

        let parameters = parameter_names
            .iter()
            .map(|name| {
                let scalar = parameter_types
                    .get(name)
                    .map(|type_name| sql_parameter_null(type_name))
                    .unwrap_or(ScalarValue::Null);
                (name.clone(), scalar)
            })
            .collect::<HashMap<_, _>>();
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let policy_forms = selected_forms
            .iter()
            .map(|form| {
                let relation = sql_relation_name(form.id);
                let scope = relation_scopes
                    .get(&relation)
                    .cloned()
                    .ok_or_else(|| anyhow!("Composition Form scope is unavailable"))?;
                Ok((
                    form.id,
                    AuthorizedQueryForm {
                        relation,
                        sql_aliases: BTreeSet::new(),
                        entry_scope: scope,
                        columns: form
                            .fields
                            .iter()
                            .map(|field| sql_column_name(field.id))
                            .collect(),
                        system_columns: BTreeSet::from([
                            QuerySystemColumn::ExternalId,
                            QuerySystemColumn::Tags,
                            QuerySystemColumn::CreatedAt,
                            QuerySystemColumn::UpdatedAt,
                        ]),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let context = workspace
            .authorized_query_context(AuthorizedQueryPolicy {
                forms: policy_forms,
                checkpoint: None,
                limits: QueryLimits {
                    max_memory_bytes: crate::index::SQL_MAX_MEMORY_BYTES,
                    max_rows: crate::index::SQL_MAX_OUTPUT_ROWS,
                    timeout: Duration::from_secs(30),
                    max_concurrency: 1,
                    function_policy: SqlFunctionPolicy::ReadOnlyAggregateV1,
                },
            })
            .await?;
        let result_schema = context
            .stateless_result_schema(&bound_sql, parameters)
            .await?;
        let result_columns = result_schema
            .iter()
            .map(|column| VariableGraphColumn {
                name: column.name.clone(),
                value_type: VariableGraphScalarDescriptor {
                    scalar_type: variable_graph_scalar_type(column.column_type),
                    nullable: column.nullable,
                },
            })
            .collect::<Vec<_>>();
        if !query_result_schema_is_well_formed(&result_columns) {
            return Ok(None);
        }
        let schema_fingerprint = query_schema_fingerprint(&result_columns)?;
        Ok(Some(PlannedSqlQueryMetadata {
            bound_sql,
            form_bindings: bindings,
            parameter_names,
            result_columns,
            schema_fingerprint,
        }))
    }

    async fn composition_form_snapshot(
        &self,
        space_id: &str,
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<(Vec<FormDefinition>, BTreeMap<String, EntryScope>)> {
        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_workspace_read_only(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        let forms = workspace
            .list_forms_bounded(
                MAX_AUTHORIZED_SCOPE_FORMS,
                MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES,
            )
            .await?;
        let scopes = match (principal_ids, authorization_state) {
            (Some(principal_ids), Some(state)) => {
                Self::authorized_form_entry_scopes_for_forms(state, principal_ids, forms.clone())?
            }
            (Some(principal_ids), None) => {
                self.authorized_form_entry_scopes_for_principals(space_id, principal_ids)
                    .await?
            }
            (None, _) => forms
                .iter()
                .map(|form| (form.name.to_ascii_lowercase(), EntryScope::AllCurrent))
                .collect(),
        };
        let forms = forms
            .into_iter()
            .filter(|form| scopes.contains_key(&form.name.to_ascii_lowercase()))
            .collect();
        Ok((forms, scopes))
    }

    /// Resolve an exact Composition revision in the principal-free local Core
    /// path. Query metadata is read from current Space state; query variables
    /// remain for the caller to execute through their bounded query methods.
    pub async fn resolve_composition_local(
        &self,
        space_id: &str,
        composition_id: &str,
        revision_id: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
    ) -> Result<CompositionResolution> {
        let raw = self
            .get_composition_raw_revision_local(space_id, composition_id, revision_id)
            .await?;
        self.resolve_composition_from_raw(space_id, raw, variables, unset_variables, None, None)
            .await
    }

    /// Resolve an exact Composition revision after rechecking current
    /// Space/Composition/query authorization for the supplied principals.
    /// This is the same semantic entrypoint used by server adapters.
    pub async fn resolve_composition_authorized_for_principals(
        &self,
        space_id: &str,
        composition_id: &str,
        revision_id: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
    ) -> Result<CompositionResolution> {
        self.resolve_composition_authorized_for_query_selection(
            space_id,
            composition_id,
            revision_id,
            variables,
            unset_variables,
            principal_ids,
            None,
        )
        .await
    }

    /// Resolve an exact authorized Composition revision while deriving query
    /// metadata only for the requested variables and their transitive query
    /// dependencies.
    pub async fn resolve_composition_authorized_for_query_selection(
        &self,
        space_id: &str,
        composition_id: &str,
        revision_id: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
        requested_queries: Option<&[String]>,
    ) -> Result<CompositionResolution> {
        let raw = self
            .get_composition_raw_revision_authorized_for_principals(
                space_id,
                composition_id,
                revision_id,
                principal_ids,
            )
            .await?;
        self.resolve_composition_from_raw(
            space_id,
            raw,
            variables,
            unset_variables,
            Some(principal_ids),
            requested_queries,
        )
        .await
    }

    async fn resolve_composition_from_raw(
        &self,
        space_id: &str,
        raw: crate::composition::RawCompositionRevision,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: Option<&[Uuid]>,
        requested_queries: Option<&[String]>,
    ) -> Result<CompositionResolution> {
        let Some(yaml) = raw.fields.get("spec").and_then(Value::as_str) else {
            return Ok(diagnostic_result(
                CompositionDiagnosticCode::InvalidComposition,
                None,
            ));
        };
        let document = match parse_composition_yaml(yaml) {
            Ok(document) => document,
            Err(code) => return Ok(diagnostic_result(code, None)),
        };
        let variable_definitions = Some(ordered_variable_definitions(&document.spec));

        let variable_bindings =
            bind_variables_with_unset(&document.spec.variables, variables, unset_variables);
        if !variable_bindings.diagnostics.is_empty() {
            return Ok(CompositionResolution {
                variable_definitions,
                plan: None,
                diagnostics: variable_bindings.diagnostics,
            });
        }

        let selected_query_ids =
            query_selection_for_resolution(&document.spec.variables, requested_queries);
        let owned_queries = self
            .read_current_queries(
                space_id,
                &document.spec.variables,
                principal_ids,
                None,
                false,
                selected_query_ids.as_ref(),
            )
            .await?;
        let current_queries = owned_queries
            .iter()
            .map(OwnedCurrentQueryDescriptor::as_current_query)
            .collect::<Vec<_>>();
        let result = resolve_composition_with_query_selection(
            ResolveInput {
                composition_revision: CompositionRevisionRef {
                    entry_id: raw.revision.entry_id,
                    revision_id: raw.revision.revision_id,
                },
                execution_id: Uuid::new_v4(),
                spec: &document.spec,
                variables,
                current_queries: &current_queries,
            },
            unset_variables,
            selected_query_ids.as_ref(),
        );

        Ok(match result {
            Ok(plan) => CompositionResolution {
                variable_definitions,
                plan: Some(plan),
                diagnostics: Vec::new(),
            },
            Err(diagnostics) => CompositionResolution {
                variable_definitions,
                plan: None,
                diagnostics,
            },
        })
    }

    /// Preview one unsaved candidate document in the principal-free local
    /// path. Current source descriptors come from current Space state; the
    /// plan carries the draft fingerprint and is never persisted.
    pub async fn preview_composition_local(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
    ) -> Result<CompositionPreview> {
        self.preview_composition_local_with_base(space_id, yaml, variables, unset_variables, None)
            .await
    }

    /// Preview a local draft and, when editing, obtain retained SQL Form
    /// bindings only from the exact Composition base revision.
    pub async fn preview_composition_local_with_base(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        base: Option<CompositionRevisionRef>,
    ) -> Result<CompositionPreview> {
        self.preview_composition_from_yaml(
            space_id,
            yaml,
            variables,
            unset_variables,
            None,
            base,
            None,
            true,
        )
        .await
    }

    /// Preview one unsaved candidate document after rechecking current
    /// Space/source authorization for the supplied principals. This is the
    /// same semantic entrypoint used by server adapters.
    pub async fn preview_composition_authorized_for_principals(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
    ) -> Result<CompositionPreview> {
        self.preview_composition_authorized_for_principals_with_base(
            space_id,
            yaml,
            variables,
            unset_variables,
            principal_ids,
            None,
        )
        .await
    }

    /// Preview an authorized draft. An optional exact base Composition
    /// revision preserves stable Form IDs across Form renames while all
    /// current Form and source authorization is rechecked.
    pub async fn preview_composition_authorized_for_principals_with_base(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
        base: Option<CompositionRevisionRef>,
    ) -> Result<CompositionPreview> {
        self.preview_composition_authorized_for_principals_with_base_and_query_selection(
            space_id,
            yaml,
            variables,
            unset_variables,
            principal_ids,
            base,
            None,
        )
        .await
    }

    /// Authoring-only preview that resolves authorized query metadata while
    /// required Inputs remain unset. It never executes SQL or stores values.
    pub async fn preview_composition_authoring_authorized_for_principals_with_base(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
        base: Option<CompositionRevisionRef>,
    ) -> Result<CompositionPreview> {
        self.preview_composition_from_yaml(
            space_id,
            yaml,
            variables,
            unset_variables,
            Some(principal_ids),
            base,
            None,
            true,
        )
        .await
    }

    /// Preview an authorized draft while deriving query metadata only for
    /// the requested variables and their transitive query dependencies.
    pub async fn preview_composition_authorized_for_principals_with_base_and_query_selection(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: &[Uuid],
        base: Option<CompositionRevisionRef>,
        requested_queries: Option<&[String]>,
    ) -> Result<CompositionPreview> {
        self.preview_composition_from_yaml(
            space_id,
            yaml,
            variables,
            unset_variables,
            Some(principal_ids),
            base,
            requested_queries,
            false,
        )
        .await
    }

    async fn preview_composition_from_yaml(
        &self,
        space_id: &str,
        yaml: &str,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: Option<&[Uuid]>,
        base: Option<CompositionRevisionRef>,
        requested_queries: Option<&[String]>,
        allow_unset_inputs: bool,
    ) -> Result<CompositionPreview> {
        let document = match parse_composition_yaml(yaml) {
            Ok(document) => document,
            Err(code) => {
                return Ok(CompositionPreview {
                    draft_fingerprint: None,
                    variable_definitions: None,
                    plan: None,
                    diagnostics: vec![preview_diagnostic(code, None)],
                });
            }
        };
        self.preview_composition_document(
            space_id,
            &document,
            variables,
            unset_variables,
            principal_ids,
            base,
            requested_queries,
            allow_unset_inputs,
        )
        .await
    }

    async fn preview_composition_document(
        &self,
        space_id: &str,
        document: &CompositionDocument,
        variables: &BTreeMap<String, Value>,
        unset_variables: &[String],
        principal_ids: Option<&[Uuid]>,
        base: Option<CompositionRevisionRef>,
        requested_queries: Option<&[String]>,
        allow_unset_inputs: bool,
    ) -> Result<CompositionPreview> {
        let draft_fingerprint = match canonicalize_composition(document) {
            Ok(canonical) => canonical.fingerprint,
            Err(code) => {
                return Ok(CompositionPreview {
                    draft_fingerprint: None,
                    variable_definitions: None,
                    plan: None,
                    diagnostics: vec![preview_diagnostic(code, None)],
                });
            }
        };
        let variable_definitions = Some(ordered_variable_definitions(&document.spec));
        let previous_document = if let Some(base) = base {
            let raw = match principal_ids {
                Some(principal_ids) => {
                    self.get_composition_raw_revision_authorized_for_principals(
                        space_id,
                        &base.entry_id.to_string(),
                        &base.revision_id.to_string(),
                        principal_ids,
                    )
                    .await?
                }
                None => {
                    self.get_composition_raw_revision_local(
                        space_id,
                        &base.entry_id.to_string(),
                        &base.revision_id.to_string(),
                    )
                    .await?
                }
            };
            let Some(previous_yaml) = raw.fields.get("spec").and_then(Value::as_str) else {
                return Ok(CompositionPreview {
                    draft_fingerprint: Some(draft_fingerprint),
                    variable_definitions,
                    plan: None,
                    diagnostics: vec![preview_diagnostic(
                        CompositionDiagnosticCode::InvalidComposition,
                        None,
                    )],
                });
            };
            match parse_composition_yaml(previous_yaml) {
                Ok(previous_document) => Some(previous_document),
                Err(code) => {
                    return Ok(CompositionPreview {
                        draft_fingerprint: Some(draft_fingerprint),
                        variable_definitions,
                        plan: None,
                        diagnostics: vec![preview_diagnostic(code, None)],
                    });
                }
            }
        } else {
            None
        };
        let selected_query_ids =
            query_selection_for_resolution(&document.spec.variables, requested_queries);
        let owned_queries = self
            .read_current_queries(
                space_id,
                &document.spec.variables,
                principal_ids,
                previous_document.as_ref(),
                true,
                selected_query_ids.as_ref(),
            )
            .await?;
        let current_queries = owned_queries
            .iter()
            .map(OwnedCurrentQueryDescriptor::as_current_query)
            .collect::<Vec<_>>();
        let execution_id = Uuid::new_v4();
        let result = if allow_unset_inputs {
            resolve_composition_authoring_preview_with_query_selection(
                execution_id,
                &document.spec,
                variables,
                unset_variables,
                &current_queries,
                draft_fingerprint.clone(),
                selected_query_ids.as_ref(),
            )
        } else {
            resolve_composition_preview_with_query_selection(
                execution_id,
                &document.spec,
                variables,
                unset_variables,
                &current_queries,
                draft_fingerprint.clone(),
                selected_query_ids.as_ref(),
            )
        };

        Ok(match result {
            Ok(plan) => CompositionPreview {
                draft_fingerprint: Some(draft_fingerprint),
                variable_definitions,
                plan: Some(plan),
                diagnostics: Vec::new(),
            },
            Err(diagnostics) => CompositionPreview {
                draft_fingerprint: Some(draft_fingerprint),
                variable_definitions,
                plan: None,
                diagnostics,
            },
        })
    }

    /// Read current query descriptors through the caller's authorization
    /// boundary, shared by saved-revision resolution and draft preview.
    async fn read_current_queries(
        &self,
        space_id: &str,
        variables: &[VariableGraphNode],
        principal_ids: Option<&[Uuid]>,
        previous_document: Option<&CompositionDocument>,
        allow_unbound_draft: bool,
        selected_query_ids: Option<&BTreeSet<String>>,
    ) -> Result<Vec<OwnedCurrentQueryDescriptor>> {
        let mut owned = Vec::new();
        let mut planning_variables = variables.to_vec();
        let Ok(query_order) = variable_graph_topological_order(&planning_variables) else {
            return Ok(Vec::new());
        };
        for variable_id in query_order {
            let Some(node_index) = planning_variables
                .iter()
                .position(|node| node.id == variable_id)
            else {
                continue;
            };
            let node = planning_variables[node_index].clone();
            let VariableGraphNodeKind::Query { query, .. } = &node.kind else {
                continue;
            };
            if selected_query_ids.is_some_and(|selected| !selected.contains(&node.id)) {
                continue;
            }
            match &query.definition {
                VariableGraphQueryDefinition::InlineSql { sql } => {
                    let trusted_bindings = if allow_unbound_draft {
                        let (readable_forms, _) = self
                            .composition_form_snapshot(space_id, principal_ids, None)
                            .await?;
                        preview_inline_form_bindings(
                            &node.id,
                            sql,
                            previous_document,
                            &readable_forms,
                        )?
                    } else {
                        Some(query.form_bindings.clone())
                    };
                    let schema = if let Some(trusted_bindings) = trusted_bindings {
                        self.derive_inline_sql_query_metadata(
                            space_id,
                            &node,
                            Some(&trusted_bindings),
                            &planning_variables,
                            principal_ids,
                            None,
                        )
                        .await?
                    } else {
                        None
                    };
                    owned.push(OwnedCurrentQueryDescriptor::InlineSql {
                        variable_id: node.id.clone(),
                        schema: schema.clone(),
                    });
                    if let Some(metadata) = schema.as_ref() {
                        if let VariableGraphNodeKind::Query { returns, query } =
                            &mut planning_variables[node_index].kind
                        {
                            if let Some(inferred) =
                                returns_from_schema(returns, &metadata.result_columns)
                            {
                                *returns = inferred;
                            }
                            query.form_bindings = metadata.form_bindings.clone();
                            query.schema_fingerprint = metadata.schema_fingerprint.clone();
                        }
                    }
                }
                VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some(entry_id),
                    revision_id: Some(revision_id),
                    ..
                } => {
                    let revision = self
                        .derive_saved_sql_query_metadata(space_id, &node, principal_ids, None)
                        .await?;
                    // The query node carries the exact immutable revision.
                    // No latest-revision lookup is available on this path.
                    if revision.as_ref().is_some_and(|current| {
                        current.entry_id != *entry_id || current.revision_id != *revision_id
                    }) {
                        owned.push(OwnedCurrentQueryDescriptor::SavedSql {
                            variable_id: node.id.clone(),
                            revision: None,
                        });
                    } else {
                        owned.push(OwnedCurrentQueryDescriptor::SavedSql {
                            variable_id: node.id.clone(),
                            revision: revision.clone(),
                        });
                        if let Some(metadata) = revision.as_ref() {
                            if let VariableGraphNodeKind::Query { returns, query } =
                                &mut planning_variables[node_index].kind
                            {
                                if let Some(inferred) =
                                    returns_from_schema(returns, &metadata.result_columns)
                                {
                                    *returns = inferred;
                                }
                                query.form_bindings = metadata.form_bindings.clone();
                                query.schema_fingerprint = metadata.schema_fingerprint.clone();
                            }
                        }
                    }
                }
                VariableGraphQueryDefinition::SavedSql { .. } => {
                    owned.push(OwnedCurrentQueryDescriptor::SavedSql {
                        variable_id: node.id.clone(),
                        revision: None,
                    });
                }
            }
        }
        Ok(owned)
    }
}

/// Resolve inline-query Form identities from the exact edit base and current
/// readable Forms. Submitted descriptors never choose these identities.
fn preview_inline_form_bindings(
    variable_id: &str,
    sql: &str,
    previous_document: Option<&CompositionDocument>,
    readable_forms: &[FormDefinition],
) -> Result<Option<Vec<VariableGraphFormBinding>>> {
    let relations = crate::index::quoted_form_name_references(sql)?;
    let previous_bindings = previous_document
        .and_then(|document| {
            document
                .spec
                .variables
                .iter()
                .find(|node| node.id == variable_id)
        })
        .and_then(|node| match &node.kind {
            VariableGraphNodeKind::Query { query, .. } => Some(query.form_bindings.as_slice()),
            _ => None,
        })
        .unwrap_or_default();
    let mut bindings = Vec::with_capacity(relations.len());
    for relation in relations {
        if let Some(binding) = previous_bindings
            .iter()
            .find(|binding| binding.relation == relation)
        {
            if !readable_forms.iter().any(|form| form.id == binding.form_id) {
                return Ok(None);
            }
            bindings.push(binding.clone());
            continue;
        }
        let Some(form) = readable_forms
            .iter()
            .find(|form| form.name == relation || sql_relation_name(form.id) == relation)
        else {
            return Ok(None);
        };
        bindings.push(VariableGraphFormBinding {
            relation,
            form_id: form.id,
        });
    }
    Ok(Some(bindings))
}

fn preview_diagnostic(
    code: CompositionDiagnosticCode,
    variable_id: Option<String>,
) -> CompositionDiagnostic {
    CompositionDiagnostic { code, variable_id }
}

struct PlannedSqlQueryMetadata {
    bound_sql: String,
    form_bindings: Vec<VariableGraphFormBinding>,
    parameter_names: BTreeSet<String>,
    result_columns: Vec<VariableGraphColumn>,
    schema_fingerprint: String,
}

fn saved_sql_revision_descriptor(
    source: &SavedSqlRevisionRef,
    revision: &saved_sql::ResolvedSavedSqlRevision,
) -> Result<saved_sql::SavedSqlRevisionDescriptor> {
    let mut variables = BTreeMap::new();
    for variable in revision.variables.as_array().into_iter().flatten() {
        let name = variable
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Saved SQL variable name is missing"))?;
        let var_type = variable
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Saved SQL variable type is missing"))?;
        variables.insert(
            name.to_owned(),
            saved_sql::SavedSqlVariableDescriptor {
                var_type: var_type.to_owned(),
            },
        );
    }
    Ok(saved_sql::SavedSqlRevisionDescriptor {
        id: source.id.clone(),
        revision_id: source.revision_id.clone(),
        variables,
    })
}

fn variable_graph_sql_type(variable_type: VariableGraphScalarType) -> &'static str {
    match variable_type {
        VariableGraphScalarType::String => "string",
        VariableGraphScalarType::Boolean => "boolean",
        VariableGraphScalarType::Integer => "integer",
        VariableGraphScalarType::Float => "float",
        VariableGraphScalarType::Decimal => "decimal",
        VariableGraphScalarType::Date => "date",
        VariableGraphScalarType::Timestamp => "timestamp",
        VariableGraphScalarType::Json => "json",
    }
}

fn variable_graph_scalar_type_for_sql(type_name: &str) -> Option<VariableGraphScalarType> {
    match type_name {
        "string" => Some(VariableGraphScalarType::String),
        "boolean" => Some(VariableGraphScalarType::Boolean),
        "integer" | "int64" | "long" => Some(VariableGraphScalarType::Integer),
        "float" | "float64" | "double" => Some(VariableGraphScalarType::Float),
        "decimal" => Some(VariableGraphScalarType::Decimal),
        "date" => Some(VariableGraphScalarType::Date),
        "timestamp" | "timestamp_tz" | "timestamp_ns" | "timestamp_tz_ns" => {
            Some(VariableGraphScalarType::Timestamp)
        }
        "json" => Some(VariableGraphScalarType::Json),
        _ => None,
    }
}

fn query_parameter_types(
    arguments: &[ugoite_domain::composition::VariableGraphArgumentBinding],
    variables: &[VariableGraphNode],
) -> Result<BTreeMap<String, String>> {
    let variables = variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    arguments
        .iter()
        .map(|argument| {
            let node = variables
                .get(argument.variable.as_str())
                .ok_or_else(|| anyhow!("query argument variable is unavailable"))?;
            let value_type = match &node.kind {
                VariableGraphNodeKind::Constant { value_type, .. }
                | VariableGraphNodeKind::Input { value_type, .. } => *value_type,
                VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Scalar { value_type },
                    ..
                } => *value_type,
                VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Table { .. },
                    ..
                } => return Err(anyhow!("table query cannot bind a scalar SQL argument")),
            };
            if matches!(
                value_type.scalar_type,
                VariableGraphScalarType::Decimal | VariableGraphScalarType::Json
            ) {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "Composition SQL arguments do not support decimal or json values",
                )
                .into());
            }
            Ok((
                argument.name.clone(),
                variable_graph_sql_type(value_type.scalar_type).to_owned(),
            ))
        })
        .collect()
}

fn sql_parameter_null(type_name: &str) -> ScalarValue {
    match type_name {
        "string" => ScalarValue::Utf8(None),
        "boolean" => ScalarValue::Boolean(None),
        "integer" | "int64" | "long" => ScalarValue::Int64(None),
        "int32" => ScalarValue::Int32(None),
        "float" | "float64" | "double" => ScalarValue::Float64(None),
        "float32" => ScalarValue::Float32(None),
        "date" => ScalarValue::Date32(None),
        "timestamp" => ScalarValue::TimestampMicrosecond(None, None),
        "timestamp_tz" => {
            ScalarValue::TimestampMicrosecond(None, Some(std::sync::Arc::from("+00:00")))
        }
        "timestamp_ns" => ScalarValue::TimestampNanosecond(None, None),
        "timestamp_tz_ns" => {
            ScalarValue::TimestampNanosecond(None, Some(std::sync::Arc::from("+00:00")))
        }
        _ => ScalarValue::Null,
    }
}

fn variable_graph_scalar_type(column_type: SqlResultColumnType) -> VariableGraphScalarType {
    match column_type {
        SqlResultColumnType::String => VariableGraphScalarType::String,
        SqlResultColumnType::Boolean => VariableGraphScalarType::Boolean,
        SqlResultColumnType::Integer => VariableGraphScalarType::Integer,
        SqlResultColumnType::Float => VariableGraphScalarType::Float,
        SqlResultColumnType::Decimal => VariableGraphScalarType::Decimal,
        SqlResultColumnType::Date => VariableGraphScalarType::Date,
        SqlResultColumnType::Timestamp => VariableGraphScalarType::Timestamp,
        SqlResultColumnType::Json => VariableGraphScalarType::Json,
    }
}

fn query_result_schema_is_well_formed(columns: &[VariableGraphColumn]) -> bool {
    !columns.is_empty()
        && columns.len() <= MAX_SQL_OUTPUT_COLUMNS
        && columns.iter().all(|column| {
            !column.name.trim().is_empty() && column.name.len() <= MAX_SQL_COLUMN_NAME_BYTES
        })
        && columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == columns.len()
}

fn query_schema_fingerprint(columns: &[VariableGraphColumn]) -> Result<String> {
    let canonical = serde_json::to_vec(columns)?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

pub(super) fn reconcile_query_returns_with_schema(
    returns: &mut VariableGraphResultType,
    result_columns: &[VariableGraphColumn],
    draft_descriptor: bool,
) -> Result<()> {
    let derived = ugoite_core::composition::returns_from_schema(returns, result_columns)
        .ok_or_else(|| {
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition scalar query must resolve to exactly one result column",
            )
        })?;
    if draft_descriptor {
        *returns = derived;
        return Ok(());
    }
    if !ugoite_core::composition::returns_match_schema(returns, result_columns) {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "Composition query result descriptor does not match current planner schema",
        )
        .into());
    }
    Ok(())
}

/// Expose declared variables with layout-placed controls first in layout
/// row and item order. An invalid placement falls back to declaration order
/// so diagnostics responses still describe the same semantic variables.
fn ordered_variable_definitions(
    spec: &ugoite_domain::composition::CompositionSpec,
) -> Vec<VariableGraphNode> {
    spec.variables.clone()
}

fn diagnostic_result(
    code: CompositionDiagnosticCode,
    variable_id: Option<String>,
) -> CompositionResolution {
    CompositionResolution {
        variable_definitions: None,
        plan: None,
        diagnostics: vec![CompositionDiagnostic { code, variable_id }],
    }
}

fn conceal_sql_query_lookup<T>(result: Result<T>, concealed_code: ErrorCode) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<AppError>()
                    .is_some_and(|app| app.code() == concealed_code)
            }) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod graph_metadata_tests {
    use super::*;

    fn form(id: FormId, name: &str) -> FormDefinition {
        FormDefinition {
            id,
            version: ugoite_domain::form::FormVersion::new(1).expect("valid version"),
            name: name.to_owned(),
            description: None,
            fields: Vec::new(),
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        }
    }

    fn exact_base_with_form_binding(form_id: FormId) -> CompositionDocument {
        CompositionDocument {
            format: ugoite_domain::composition::CompositionFormat::UgoiteComposition,
            format_version: 1,
            kind: ugoite_domain::composition::CompositionKind::Dashboard,
            name: "Base".to_owned(),
            spec: ugoite_domain::composition::CompositionSpec {
                variables: vec![VariableGraphNode {
                    id: "rows".to_owned(),
                    kind: VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Table {
                            columns: Vec::new(),
                        },
                        query: ugoite_domain::composition::VariableGraphQuery {
                            definition: VariableGraphQueryDefinition::InlineSql {
                                sql: "SELECT * FROM \"Original\"".to_owned(),
                            },
                            arguments: Vec::new(),
                            form_bindings: vec![VariableGraphFormBinding {
                                relation: "Original".to_owned(),
                                form_id,
                            }],
                            schema_fingerprint: "a".repeat(64),
                        },
                    },
                }],
                components: Vec::new(),
                layout: ugoite_domain::composition::DashboardFlowLayout {
                    kind: ugoite_domain::composition::FlowLayoutKind::Flow,
                    rows: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn exact_base_form_binding_survives_rename_and_fails_closed_after_recreate() {
        let original_id = FormId::from_uuid(Uuid::from_u128(10));
        let base = exact_base_with_form_binding(original_id);
        let sql = "SELECT * FROM \"Original\"";

        let renamed = form(original_id, "Renamed");
        assert_eq!(
            preview_inline_form_bindings("rows", sql, Some(&base), &[renamed])
                .expect("rename resolution"),
            Some(vec![VariableGraphFormBinding {
                relation: "Original".to_owned(),
                form_id: original_id,
            }])
        );

        let recreated = form(FormId::from_uuid(Uuid::from_u128(11)), "Original");
        assert_eq!(
            preview_inline_form_bindings("rows", sql, Some(&base), &[recreated])
                .expect("recreate resolution"),
            None,
            "a same-name replacement must not receive the exact base binding"
        );
    }

    #[test]
    fn scalar_query_result_must_have_exactly_one_planner_column() {
        let value_type = VariableGraphScalarDescriptor {
            scalar_type: VariableGraphScalarType::Integer,
            nullable: false,
        };
        let mut returns = VariableGraphResultType::Scalar { value_type };
        let result_columns = vec![
            VariableGraphColumn {
                name: "first".to_owned(),
                value_type,
            },
            VariableGraphColumn {
                name: "second".to_owned(),
                value_type,
            },
        ];

        let error = reconcile_query_returns_with_schema(&mut returns, &result_columns, true)
            .expect_err("scalar result shape must not silently select a column");
        assert_eq!(
            error.downcast_ref::<AppError>().map(AppError::code),
            Some(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn query_argument_sql_type_is_derived_from_graph_scalar_type() {
        assert_eq!(
            variable_graph_sql_type(VariableGraphScalarType::String),
            "string"
        );
        assert_eq!(
            variable_graph_sql_type(VariableGraphScalarType::Integer),
            "integer"
        );
        assert_eq!(
            variable_graph_sql_type(VariableGraphScalarType::Decimal),
            "decimal"
        );
    }
}
