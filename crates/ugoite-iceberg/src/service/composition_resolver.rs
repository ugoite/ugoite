//! Storage-backed Composition resolution shared by local and server clients.
//!
//! The service reads one exact Composition revision and current source
//! descriptors, then delegates binding and query compilation to
//! `ugoite_core::composition`. It does not execute queries or persist runtime
//! state.

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_core::composition::{
    bind_parameters, resolve_composition, CompositionDiagnostic, CompositionRevisionRef,
    CurrentSourceDescriptor, ResolveInput, ResolvedCompositionPlan, SavedSqlRevisionMetadata,
};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::sql_query::SavedSqlRevisionRef;
use ugoite_domain::composition::{
    parse_composition_yaml, CompositionDiagnosticCode, CompositionParameter, CompositionSource,
};
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
/// boundary. Parameter definitions are present once the YAML envelope parses,
/// including when supplied parameter values are invalid.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionResolution {
    pub parameter_definitions: Option<Vec<CompositionParameter>>,
    pub plan: Option<ResolvedCompositionPlan>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

enum OwnedCurrentSourceDescriptor {
    EntryQuery {
        source_id: String,
        form: Option<ugoite_domain::form::FormDefinition>,
    },
    SavedSql {
        source_id: String,
        revision: Option<SavedSqlRevisionMetadata>,
    },
}

impl OwnedCurrentSourceDescriptor {
    fn as_current_source(&self) -> CurrentSourceDescriptor<'_> {
        match self {
            Self::EntryQuery { source_id, form } => CurrentSourceDescriptor::EntryQuery {
                source_id,
                current_form: form.as_ref(),
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

    /// Resolve an exact Composition revision in the principal-free local Core
    /// path. Source metadata is read from current Space state; source queries
    /// remain for the caller to execute through their bounded query methods.
    pub async fn resolve_composition_local(
        &self,
        space_id: &str,
        composition_id: &str,
        revision_id: &str,
        parameters: &BTreeMap<String, Value>,
    ) -> Result<CompositionResolution> {
        let raw = self
            .get_composition_raw_revision_local(space_id, composition_id, revision_id)
            .await?;
        self.resolve_composition_from_raw(space_id, raw, parameters, None)
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
        parameters: &BTreeMap<String, Value>,
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
        self.resolve_composition_from_raw(space_id, raw, parameters, Some(principal_ids))
            .await
    }

    async fn resolve_composition_from_raw(
        &self,
        space_id: &str,
        raw: crate::composition::RawCompositionRevision,
        parameters: &BTreeMap<String, Value>,
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
        let parameter_definitions = Some(document.spec.parameters.clone());

        let parameter_bindings = bind_parameters(&document.spec.parameters, parameters);
        if !parameter_bindings.diagnostics.is_empty() {
            return Ok(CompositionResolution {
                parameter_definitions,
                plan: None,
                diagnostics: parameter_bindings.diagnostics,
            });
        }

        let mut owned_sources = Vec::with_capacity(document.spec.sources.len());
        for source in &document.spec.sources {
            match source {
                CompositionSource::EntryQuery { id, form_id, .. } => {
                    let form = match principal_ids {
                        Some(principal_ids) => conceal_source_lookup(
                            self.get_composition_source_form_authorized_for_principals(
                                space_id,
                                *form_id,
                                principal_ids,
                            )
                            .await,
                            ErrorCode::FormNotFound,
                        )?,
                        None => {
                            self.get_composition_source_form_local(space_id, *form_id)
                                .await?
                        }
                    };
                    owned_sources.push(OwnedCurrentSourceDescriptor::EntryQuery {
                        source_id: id.clone(),
                        form,
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
                    let descriptor = match principal_ids {
                        Some(principal_ids) => conceal_source_lookup(
                            self.get_saved_sql_revision_descriptor_authorized_for_principals(
                                space_id,
                                &saved_sql_ref,
                                principal_ids,
                            )
                            .await,
                            ErrorCode::EntryNotFound,
                        )?,
                        None => {
                            self.get_saved_sql_revision_descriptor_local(space_id, &saved_sql_ref)
                                .await?
                        }
                    }
                    .map(|descriptor| SavedSqlRevisionMetadata {
                        id: descriptor.id,
                        revision_id: descriptor.revision_id,
                        variable_types: descriptor
                            .variables
                            .into_iter()
                            .map(|(name, variable)| (name, variable.var_type))
                            .collect(),
                    });
                    owned_sources.push(OwnedCurrentSourceDescriptor::SavedSql {
                        source_id: id.clone(),
                        revision: descriptor,
                    });
                }
            }
        }

        let current_sources = owned_sources
            .iter()
            .map(OwnedCurrentSourceDescriptor::as_current_source)
            .collect::<Vec<_>>();
        let result = resolve_composition(ResolveInput {
            composition_revision: CompositionRevisionRef {
                entry_id: raw.revision.entry_id,
                revision_id: raw.revision.revision_id,
            },
            spec: &document.spec,
            parameters,
            current_sources: &current_sources,
        });

        Ok(match result {
            Ok(plan) => CompositionResolution {
                parameter_definitions,
                plan: Some(plan),
                diagnostics: Vec::new(),
            },
            Err(diagnostics) => CompositionResolution {
                parameter_definitions,
                plan: None,
                diagnostics,
            },
        })
    }
}

fn diagnostic_result(
    code: CompositionDiagnosticCode,
    parameter_id: Option<String>,
) -> CompositionResolution {
    CompositionResolution {
        parameter_definitions: None,
        plan: None,
        diagnostics: vec![CompositionDiagnostic { code, parameter_id }],
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
        let yaml = format!(
            "format: ugoite.composition\nformat_version: 1\nkind: dashboard\nname: Local query\ntags: []\nspec:\n  sources:\n    - id: rows\n      kind: entry_query\n      form_id: \"{}\"\n      field_schema:\n        - field_id: 100\n          field_type: string\n      query:\n        projection:\n          kind: fields\n          fields: [100]\n  components:\n    - id: table\n      kind: table\n      source: rows\n  sections:\n    - id: main\n      components: [table]\n",
            form.form_id
        );
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
            )
            .await?;
        let plan = resolution.plan.expect("current Form resolves");
        assert_eq!(plan.composition_revision.entry_id, saved.entry_id);
        assert_eq!(plan.composition_revision.revision_id, saved.revision_id);
        assert_eq!(plan.sources.len(), 1);

        let missing_revision = Uuid::now_v7().to_string();
        let missing = service
            .resolve_composition_local(
                &space_id,
                &saved.entry_id.to_string(),
                &missing_revision,
                &BTreeMap::new(),
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
