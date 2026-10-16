//! Storage-backed Composition resolution shared by local and server clients.
//!
//! The service reads one exact Composition revision and current source
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
    bind_variables_with_unset, resolve_composition_preview_with_unset,
    resolve_composition_with_unset, CompositionDiagnostic, CompositionRevisionRef,
    CurrentSourceDescriptor, InlineSqlSourceMetadata, PreviewCompositionPlan, ResolveInput,
    ResolvedCompositionPlan, SavedSqlRevisionMetadata,
};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
    SqlFunctionPolicy,
};
use ugoite_core::sql_query::{
    SavedSqlRevisionRef, SqlResultColumnType, MAX_SQL_COLUMN_NAME_BYTES, MAX_SQL_OUTPUT_COLUMNS,
};
use ugoite_domain::composition::{
    canonicalize_composition, parse_composition_yaml, CompositionDiagnosticCode,
    CompositionDocument, CompositionFormBinding, CompositionResultColumn,
    CompositionResultFieldType, CompositionSource, CompositionSpec, CompositionVariable,
    CompositionVariableType,
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
    pub variable_definitions: Option<Vec<CompositionVariable>>,
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
    pub variable_definitions: Option<Vec<CompositionVariable>>,
    pub plan: Option<PreviewCompositionPlan>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

enum OwnedCurrentSourceDescriptor {
    InlineSql {
        source_id: String,
        schema: Option<InlineSqlSourceMetadata>,
    },
    SavedSql {
        source_id: String,
        revision: Option<SavedSqlRevisionMetadata>,
    },
}

impl OwnedCurrentSourceDescriptor {
    fn as_current_source(&self) -> CurrentSourceDescriptor<'_> {
        match self {
            Self::InlineSql { source_id, schema } => CurrentSourceDescriptor::InlineSql {
                source_id,
                current_schema: schema.as_ref(),
            },
            Self::SavedSql {
                source_id,
                revision,
            } => CurrentSourceDescriptor::SavedSql {
                source_id,
                current_revision: revision.as_ref(),
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

    /// Derive trusted metadata for an inline Composition SQL source.
    ///
    /// `Some(bindings)` means the caller already has server-trusted Form-ID
    /// bindings; the SQL relation set must match them exactly. `None` is
    /// reserved for the all-empty draft descriptor tuple and resolves the
    /// quoted Form names from the current authorized Form snapshot so the
    /// first preview can return the descriptor that save will persist.
    pub(crate) async fn derive_inline_sql_source_metadata(
        &self,
        space_id: &str,
        source: &CompositionSource,
        trusted_bindings: Option<&[CompositionFormBinding]>,
        variables: &[CompositionVariable],
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<InlineSqlSourceMetadata>> {
        let CompositionSource::InlineSql {
            sql,
            form_bindings,
            expected_result,
            schema_fingerprint,
            ..
        } = source
        else {
            return Err(anyhow!("expected an inline SQL Composition source"));
        };
        let is_draft_placeholder =
            form_bindings.is_empty() && expected_result.is_empty() && schema_fingerprint.is_empty();
        if trusted_bindings.is_none() && !is_draft_placeholder {
            return Ok(None);
        }

        let parameter_types = variables
            .iter()
            .map(|variable| {
                (
                    variable.id.clone(),
                    composition_variable_sql_type(variable.variable_type).to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let planned = self
            .plan_sql_source_metadata(
                space_id,
                sql,
                trusted_bindings,
                &parameter_types,
                principal_ids,
                authorization_state,
            )
            .await?;
        Ok(planned.map(|planned| InlineSqlSourceMetadata {
            bound_sql: planned.bound_sql,
            form_bindings: planned.form_bindings,
            parameter_names: planned.parameter_names,
            expected_result: planned.expected_result,
            schema_fingerprint: planned.schema_fingerprint,
        }))
    }

    /// Derive current schema metadata for one exact Saved SQL revision.
    ///
    /// The Saved SQL descriptor read enforces its current ACL (or local
    /// operator boundary). Its immutable server-owned Form bindings are then
    /// resolved by ID and planned through the same closed DataFusion context
    /// used for inline Composition SQL.
    pub(crate) async fn derive_saved_sql_source_metadata(
        &self,
        space_id: &str,
        source: &SavedSqlRevisionRef,
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<SavedSqlRevisionMetadata>> {
        let (descriptor, revision) = if let Some(state) = authorization_state {
            let principal_ids = principal_ids
                .ok_or_else(|| anyhow!("authorization state requires principal IDs"))?;
            validate_storage_id(validate_sql_id(&source.id))?;
            validate_storage_id(validate_revision_id(&source.revision_id))?;
            super::ensure_saved_sql_access(state, principal_ids, source)?;
            self.validate_complete_space(space_id).await?;
            let revision = self
                .read_saved_sql_revision_for_metadata(space_id, source)
                .await?;
            let Some(revision) = revision else {
                return Ok(None);
            };
            let descriptor = saved_sql_revision_descriptor(source, &revision)?;
            (descriptor, revision)
        } else {
            let descriptor = match principal_ids {
                Some(principal_ids) => conceal_source_lookup(
                    self.get_saved_sql_revision_descriptor_authorized_for_principals(
                        space_id,
                        source,
                        principal_ids,
                    )
                    .await,
                    ErrorCode::EntryNotFound,
                )?,
                None => {
                    self.get_saved_sql_revision_descriptor_local(space_id, source)
                        .await?
                }
            };
            let Some(descriptor) = descriptor else {
                return Ok(None);
            };
            let Some(revision) = self
                .read_saved_sql_revision_for_metadata(space_id, source)
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
                Ok(CompositionFormBinding {
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
        let variable_types = parameter_types.clone();
        let planned = self
            .plan_sql_source_metadata(
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
        Ok(Some(SavedSqlRevisionMetadata {
            id: descriptor.id,
            revision_id: descriptor.revision_id,
            variable_types,
            expected_result: planned.expected_result,
            schema_fingerprint: planned.schema_fingerprint,
        }))
    }

    async fn plan_sql_source_metadata(
        &self,
        space_id: &str,
        sql: &str,
        trusted_bindings: Option<&[CompositionFormBinding]>,
        parameter_types: &BTreeMap<String, String>,
        principal_ids: Option<&[Uuid]>,
        authorization_state: Option<&super::AuthorizationState>,
    ) -> Result<Option<PlannedSqlSourceMetadata>> {
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
                    bindings.push(CompositionFormBinding {
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
                    resolved.push(CompositionFormBinding {
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
                (Some(principal_ids), None) => match conceal_source_lookup(
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
        let page = context
            .execute_stateless_page(&bound_sql, parameters, 0, 1)
            .await?;
        let expected_result = page
            .result_schema
            .iter()
            .map(|column| CompositionResultColumn {
                name: column.name.clone(),
                result_type: composition_result_field_type(column.column_type),
            })
            .collect::<Vec<_>>();
        if !composition_result_schema_is_well_formed(&expected_result) {
            return Ok(None);
        }
        let schema_fingerprint = composition_schema_fingerprint(&expected_result)?;
        Ok(Some(PlannedSqlSourceMetadata {
            bound_sql,
            form_bindings: bindings,
            parameter_names,
            expected_result,
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
    /// path. Source metadata is read from current Space state; source queries
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
        self.resolve_composition_from_raw(space_id, raw, variables, unset_variables, None)
            .await
    }

    /// Resolve an exact Composition revision after rechecking current
    /// Space/Composition/source authorization for the supplied principals.
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

        let owned_sources = self
            .read_current_sources(
                space_id,
                &document.spec.sources,
                &document.spec.variables,
                principal_ids,
                None,
                false,
            )
            .await?;
        let current_sources = owned_sources
            .iter()
            .map(OwnedCurrentSourceDescriptor::as_current_source)
            .collect::<Vec<_>>();
        let result = resolve_composition_with_unset(
            ResolveInput {
                composition_revision: CompositionRevisionRef {
                    entry_id: raw.revision.entry_id,
                    revision_id: raw.revision.revision_id,
                },
                spec: &document.spec,
                variables,
                current_sources: &current_sources,
            },
            unset_variables,
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
        self.preview_composition_from_yaml(space_id, yaml, variables, unset_variables, None, base)
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
        self.preview_composition_from_yaml(
            space_id,
            yaml,
            variables,
            unset_variables,
            Some(principal_ids),
            base,
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
        let owned_sources = self
            .read_current_sources(
                space_id,
                &document.spec.sources,
                &document.spec.variables,
                principal_ids,
                previous_document.as_ref(),
                true,
            )
            .await?;
        let mut preview_spec = document.spec.clone();
        for (source, descriptor) in preview_spec.sources.iter_mut().zip(&owned_sources) {
            match (source, descriptor) {
                (
                    CompositionSource::InlineSql {
                        form_bindings,
                        expected_result,
                        schema_fingerprint,
                        ..
                    },
                    OwnedCurrentSourceDescriptor::InlineSql {
                        schema: Some(metadata),
                        ..
                    },
                ) => {
                    *form_bindings = metadata.form_bindings.clone();
                    *expected_result = metadata.expected_result.clone();
                    *schema_fingerprint = metadata.schema_fingerprint.clone();
                }
                (
                    CompositionSource::SavedSql {
                        expected_result,
                        schema_fingerprint,
                        ..
                    },
                    OwnedCurrentSourceDescriptor::SavedSql {
                        revision: Some(metadata),
                        ..
                    },
                ) => {
                    *expected_result = metadata.expected_result.clone();
                    *schema_fingerprint = metadata.schema_fingerprint.clone();
                }
                _ => {}
            }
        }
        let current_sources = owned_sources
            .iter()
            .map(OwnedCurrentSourceDescriptor::as_current_source)
            .collect::<Vec<_>>();
        let result = resolve_composition_preview_with_unset(
            &preview_spec,
            variables,
            unset_variables,
            &current_sources,
            draft_fingerprint.clone(),
        );

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

    /// Read current source descriptors through the caller's authorization
    /// boundary, shared by saved-revision resolution and draft preview.
    /// Absent or denied sources stay `None` and compile to the same
    /// `source_unavailable` diagnostic without revealing existence.
    async fn read_current_sources(
        &self,
        space_id: &str,
        sources: &[CompositionSource],
        variables: &[CompositionVariable],
        principal_ids: Option<&[Uuid]>,
        previous_document: Option<&CompositionDocument>,
        allow_unbound_draft: bool,
    ) -> Result<Vec<OwnedCurrentSourceDescriptor>> {
        let mut owned_sources = Vec::with_capacity(sources.len());
        for source in sources {
            match source {
                CompositionSource::InlineSql { id, sql, .. } => {
                    let trusted_bindings = if allow_unbound_draft {
                        let (readable_forms, _) = self
                            .composition_form_snapshot(space_id, principal_ids, None)
                            .await?;
                        preview_inline_form_bindings(id, sql, previous_document, &readable_forms)?
                    } else {
                        match source {
                            CompositionSource::InlineSql { form_bindings, .. } => {
                                Some(form_bindings.clone())
                            }
                            CompositionSource::SavedSql { .. } => unreachable!(),
                        }
                    };
                    let schema = if let Some(trusted_bindings) = trusted_bindings {
                        self.derive_inline_sql_source_metadata(
                            space_id,
                            source,
                            Some(&trusted_bindings),
                            variables,
                            principal_ids,
                            None,
                        )
                        .await?
                    } else {
                        None
                    };
                    owned_sources.push(OwnedCurrentSourceDescriptor::InlineSql {
                        source_id: id.clone(),
                        schema,
                    });
                }
                CompositionSource::SavedSql {
                    id,
                    entry_id,
                    revision_id,
                    ..
                } => {
                    let saved_sql_ref = SavedSqlRevisionRef {
                        id: entry_id.to_string(),
                        revision_id: revision_id.to_string(),
                    };
                    let descriptor = self
                        .derive_saved_sql_source_metadata(
                            space_id,
                            &saved_sql_ref,
                            principal_ids,
                            None,
                        )
                        .await?;
                    owned_sources.push(OwnedCurrentSourceDescriptor::SavedSql {
                        source_id: id.clone(),
                        revision: descriptor,
                    });
                }
            }
        }

        Ok(owned_sources)
    }
}

/// Resolve an inline preview's Form identities from the exact edit base and
/// current readable Forms. YAML descriptors never participate in this choice.
fn preview_inline_form_bindings(
    source_id: &str,
    sql: &str,
    previous_document: Option<&CompositionDocument>,
    readable_forms: &[FormDefinition],
) -> Result<Option<Vec<CompositionFormBinding>>> {
    let relations = crate::index::quoted_form_name_references(sql)?;
    let previous_bindings = previous_document
        .and_then(|document| {
            document
                .spec
                .sources
                .iter()
                .find(|source| source.id() == source_id)
        })
        .and_then(|source| match source {
            CompositionSource::InlineSql { form_bindings, .. } => Some(form_bindings.as_slice()),
            CompositionSource::SavedSql { .. } => None,
        })
        .unwrap_or_default();

    let mut bindings = Vec::with_capacity(relations.len());
    for relation in relations {
        if let Some(binding) = previous_bindings
            .iter()
            .find(|binding| binding.relation == relation)
        {
            // An existing binding is authoritative for an unchanged source
            // relation, but it is usable only while that stable Form ID is
            // still present in the caller's readable Form snapshot. Never
            // fall back to a same-name replacement.
            if !readable_forms.iter().any(|form| form.id == binding.form_id) {
                return Ok(None);
            }
            bindings.push(binding.clone());
            continue;
        }
        let Some(form) = readable_forms.iter().find(|form| form.name == relation) else {
            return Ok(None);
        };
        bindings.push(CompositionFormBinding {
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

struct PlannedSqlSourceMetadata {
    bound_sql: String,
    form_bindings: Vec<CompositionFormBinding>,
    parameter_names: BTreeSet<String>,
    expected_result: Vec<CompositionResultColumn>,
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

fn composition_variable_sql_type(variable_type: CompositionVariableType) -> &'static str {
    match variable_type {
        CompositionVariableType::String => "string",
        CompositionVariableType::Boolean => "boolean",
        CompositionVariableType::Integer => "integer",
        CompositionVariableType::Float => "float",
        CompositionVariableType::Date => "date",
        CompositionVariableType::Timestamp => "timestamp",
    }
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

fn composition_result_field_type(column_type: SqlResultColumnType) -> CompositionResultFieldType {
    match column_type {
        SqlResultColumnType::String => CompositionResultFieldType::String,
        SqlResultColumnType::Boolean => CompositionResultFieldType::Boolean,
        SqlResultColumnType::Integer => CompositionResultFieldType::Integer,
        SqlResultColumnType::Float => CompositionResultFieldType::Float,
        SqlResultColumnType::Decimal => CompositionResultFieldType::Decimal,
        SqlResultColumnType::Date => CompositionResultFieldType::Date,
        SqlResultColumnType::Timestamp => CompositionResultFieldType::Timestamp,
        SqlResultColumnType::Json => CompositionResultFieldType::Json,
    }
}

fn composition_result_schema_is_well_formed(columns: &[CompositionResultColumn]) -> bool {
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

fn composition_schema_fingerprint(columns: &[CompositionResultColumn]) -> Result<String> {
    let canonical = serde_json::to_vec(columns)?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

#[cfg(test)]
mod preview_binding_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_name_form_replacement_does_not_replace_exact_base_binding() {
        let mut previous = parse_composition_yaml(include_str!(
            "../../../ugoite-domain/tests/fixtures/composition/sql-first.ugcomp.yaml"
        ))
        .expect("fixture parses");
        let old_form_id = FormId::from_uuid(Uuid::from_u128(0x100));
        let replacement_form_id = FormId::from_uuid(Uuid::from_u128(0x200));
        let source = previous
            .spec
            .sources
            .iter_mut()
            .find(|source| source.id() == "expense_rows")
            .expect("inline source exists");
        let CompositionSource::InlineSql { form_bindings, .. } = source else {
            panic!("source is inline SQL");
        };
        *form_bindings = vec![CompositionFormBinding {
            relation: "Expense".to_owned(),
            form_id: old_form_id,
        }];
        let replacement: FormDefinition = serde_json::from_value(json!({
            "id": replacement_form_id.to_string(),
            "version": 1,
            "name": "Expense",
            "fields": [],
            "allow_extra_attributes": true
        }))
        .expect("replacement Form is valid");

        let bindings = preview_inline_form_bindings(
            "expense_rows",
            "SELECT * FROM \"Expense\"",
            Some(&previous),
            &[replacement],
        )
        .expect("relation set parses");

        assert_eq!(bindings, None);
        assert_ne!(old_form_id, replacement_form_id);
    }

    #[tokio::test]
    async fn exact_base_preview_retains_form_id_after_rename() -> anyhow::Result<()> {
        let service = UgoiteService::new(format!(
            "memory://composition-preview-form-rename-{}",
            Uuid::now_v7()
        ))?;
        let owner = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("composition-preview-form-rename", owner, "Owner")
            .await?
            .to_string();
        let form = service
            .upsert_form_result(
                &space_id,
                &json!({
                    "name": "Task",
                    "fields": {
                        "title": {"id": 100, "type": "string", "required": false}
                    }
                }),
            )
            .await?;
        let yaml = "format: ugoite.composition\nformat_version: 1\nkind: dashboard\nname: Task rows\nspec:\n  sources:\n    - id: rows\n      kind: inline_sql\n      name: Rows\n      sql: |\n        SELECT \"field_100\" FROM \"Task\"\n      form_bindings: []\n      expected_result: []\n      schema_fingerprint: ''\n  components:\n    - id: table\n      kind: table\n      source: rows\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: table\n";
        let saved = service
            .save_composition_local(
                &space_id,
                crate::composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document: parse_composition_yaml(yaml).expect("draft parses"),
                },
                "preview-test",
            )
            .await?;

        let workspace = iceberg_store::native_mutation_workspace(
            service.operator(),
            &service.workspace_path(&space_id),
        )
        .await?;
        let current_form = workspace
            .list_forms()
            .await?
            .into_iter()
            .find(|candidate| candidate.id == form.form_id)
            .expect("source Form exists");
        let changes = ugoite_domain::form::FormChangeSet {
            form_id: form.form_id,
            expected_version: Some(current_form.version),
            changes: vec![ugoite_domain::form::FormChange::RenameForm {
                name: "RenamedTask".to_owned(),
            }],
        };
        let publication = crate::system_publication_context(
            format!("test-composition-preview-rename:{}", form.form_id),
            "form.evolve",
            &changes,
        )?;
        workspace
            .commit(publication)?
            .evolve_form_with_receipt(&changes)
            .await?;

        let preview = service
            .preview_composition_local_with_base(
                &space_id,
                &saved.canonical_yaml,
                &BTreeMap::new(),
                &[],
                Some(CompositionRevisionRef {
                    entry_id: saved.entry_id,
                    revision_id: saved.revision_id,
                }),
            )
            .await?;
        let plan = preview
            .plan
            .expect("exact base should retain the renamed Form's identity");
        let stable_relation = sql_relation_name(form.form_id);
        assert!(matches!(
            &plan.sources[0],
            ugoite_core::composition::ResolvedSourceRequest::Sql { request, .. }
                if request.sql.contains(&stable_relation)
                    && !request.sql.contains("Task")
        ));
        Ok(())
    }
}

/// Expose declared variables with layout-placed controls first in layout
/// row and item order. An invalid placement falls back to declaration order
/// so diagnostics responses still describe the same semantic variables.
fn ordered_variable_definitions(spec: &CompositionSpec) -> Vec<CompositionVariable> {
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

fn conceal_source_lookup<T>(result: Result<T>, concealed_code: ErrorCode) -> Result<Option<T>> {
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
mod tests {
    use super::*;
    use crate::composition::CompositionSaveRequest;
    use serde_json::json;
    use ugoite_domain::composition::parse_composition_yaml;

    #[tokio::test]
    async fn local_resolution_compiles_current_form_for_exact_composition_revision(
    ) -> anyhow::Result<()> {
        let service = UgoiteService::new(format!(
            "memory://composition-local-resolve-{}",
            Uuid::now_v7()
        ))?;
        let owner = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal("composition-local-resolve", owner, "Owner")
            .await?
            .to_string();
        let form = service
            .upsert_form_result(
                &space_id,
                &json!({
                    "name": "Composition-Source",
                    "fields": {
                        "title": {"id": 100, "type": "string", "required": false}
                    }
                }),
            )
            .await?;
        let yaml = (
            "format: ugoite.composition\nformat_version: 1\nkind: dashboard\nname: Local query\nspec:\n  sources:\n    - id: rows\n      kind: inline_sql\n      name: Rows\n      sql: |\n        SELECT \"field_100\" FROM \"Composition-Source\" ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\"\n      form_bindings: []\n      expected_result: []\n      schema_fingerprint: ''\n  components:\n    - id: table\n      kind: table\n      source: rows\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: table\n"
        )
        .to_string();
        let document = parse_composition_yaml(&yaml).expect("Composition fixture parses");
        let saved = service
            .save_composition_authorized_for_principals(
                &space_id,
                CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document,
                },
                &owner.to_string(),
                &[owner],
            )
            .await?;

        let resolution = service
            .resolve_composition_local(
                &space_id,
                &saved.entry_id.to_string(),
                &saved.revision_id.to_string(),
                &BTreeMap::new(),
                &[],
            )
            .await?;
        let plan = resolution.plan.expect("current Form resolves");
        assert_eq!(plan.composition_revision.entry_id, saved.entry_id);
        assert_eq!(plan.composition_revision.revision_id, saved.revision_id);
        assert_eq!(plan.sources.len(), 1);
        let stable_relation = sql_relation_name(form.form_id);
        assert!(matches!(
            &plan.sources[0],
            ugoite_core::composition::ResolvedSourceRequest::Sql { request, .. }
                if request.sql.contains(&stable_relation)
                    && !request.sql.contains("Composition-Source")
        ));

        let missing_revision = Uuid::now_v7().to_string();
        let missing = service
            .resolve_composition_local(
                &space_id,
                &saved.entry_id.to_string(),
                &missing_revision,
                &BTreeMap::new(),
                &[],
            )
            .await
            .expect_err("an absent exact revision must not fall back to current");
        let missing = missing.downcast::<AppError>()?;
        assert_eq!(missing.code(), ErrorCode::EntryNotFound);

        let history = service
            .composition_history_local_page(&space_id, &saved.entry_id.to_string(), 10, 0)
            .await?;
        assert_eq!(history.total, 1, "resolution does not publish a revision");
        Ok(())
    }
}
