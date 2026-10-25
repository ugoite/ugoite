use super::*;

pub(super) async fn read_exact_base_document(
    operator: &Operator,
    workspace_path: &str,
    entry_id: &str,
    revision_id: &str,
) -> Result<ugoite_domain::composition::CompositionDocument> {
    let raw =
        composition::read_composition_raw_revision(operator, workspace_path, entry_id, revision_id)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    ErrorCode::EntryNotFound,
                    "Composition base revision not found",
                )
            })?;
    parse_exact_base_document(&raw.fields)
}

fn parse_exact_base_document(
    fields: &BTreeMap<String, Value>,
) -> Result<ugoite_domain::composition::CompositionDocument> {
    let yaml = fields
        .get("spec")
        .and_then(Value::as_str)
        .ok_or_else(invalid_base_composition)?;
    ugoite_domain::composition::parse_composition_yaml(yaml).map_err(|_| invalid_base_composition())
}

fn invalid_base_composition() -> anyhow::Error {
    AppError::invalid_input(
        ErrorCode::InvalidInput,
        "invalid_composition: base revision is not a valid Composition document",
    )
    .into()
}

impl UgoiteService {
    /// Create or update one Composition through the principal-free local Core
    /// path. Remote adapters must use the authorized save method.
    pub async fn save_composition_local(
        &self,
        space_id: &str,
        request: composition::CompositionSaveRequest,
        author: &str,
    ) -> Result<composition::CompositionSaveResult> {
        let operation_id = Uuid::now_v7().to_string();
        self.save_composition_local_with_operation_id(space_id, request, author, &operation_id)
            .await
    }

    /// Local CLI save with an optional caller-stable retry identity. Callers
    /// that may retry after an uncertain process outcome can reuse the same
    /// identity and canonical request to recover the original receipt.
    pub async fn save_composition_local_with_operation_id(
        &self,
        space_id: &str,
        request: composition::CompositionSaveRequest,
        author: &str,
        operation_id: &str,
    ) -> Result<composition::CompositionSaveResult> {
        self.save_composition_local_with_work(
            space_id,
            request,
            author,
            operation_id,
            composition::CompositionSaveWork::default(),
        )
        .await
    }

    pub async fn save_composition_local_with_work(
        &self,
        space_id: &str,
        mut request: composition::CompositionSaveRequest,
        author: &str,
        operation_id: &str,
        work: composition::CompositionSaveWork,
    ) -> Result<composition::CompositionSaveResult> {
        composition::validate_operation_id(operation_id)?;
        if request.entry_id.is_some() != request.base_revision_id.is_some() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition updates require an entry ID and exact base revision",
            )
            .into());
        }

        // Reject unsupported or invalid documents before registry creation or
        // any other persistent side effect.
        let request_yaml = ugoite_domain::composition::canonicalize_composition(&request.document)
            .map_err(|diagnostic| {
                AppError::invalid_input(ErrorCode::InvalidInput, diagnostic.as_str())
            })?
            .yaml;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;

        // Local Core saves have no caller ACL boundary, but saved SQL sources
        // still persist only source descriptors derived from current local
        // Forms and exact Saved SQL revisions. This lets the CLI remain fully
        // usable without routing Composition saves through the server.
        let workspace_path = self.workspace_path(space_id);
        if let Some(result) = composition::recover_published_composition_save(
            &self.operator,
            space_id,
            &workspace_path,
            &request,
            &request_yaml,
            author,
            operation_id,
            &work,
        )
        .await?
        {
            return Ok(result);
        }
        let previous_document = match (request.entry_id, request.base_revision_id) {
            (Some(entry_id), Some(base_revision_id)) => Some(
                read_exact_base_document(
                    &self.operator,
                    &workspace_path,
                    &entry_id.to_string(),
                    &base_revision_id.to_string(),
                )
                .await?,
            ),
            _ => None,
        };
        let forms = iceberg_store::native_workspace_read_only(&self.operator, &workspace_path)
            .await?
            .list_forms_bounded(
                MAX_AUTHORIZED_SCOPE_FORMS,
                MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES,
            )
            .await?;
        let query_order = super::composition_resolver::variable_graph_topological_order(
            &request.document.spec.variables,
        )
        .map_err(|_| {
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition query dependencies are invalid",
            )
        })?;
        for variable_id in query_order {
            let variables = request.document.spec.variables.clone();
            let Some(node_index) = variables.iter().position(|node| node.id == variable_id) else {
                continue;
            };
            let source_for_planning = variables[node_index].clone();
            let node = &mut request.document.spec.variables[node_index];
            let ugoite_domain::composition::VariableGraphNodeKind::Query { query, returns } =
                &mut node.kind
            else {
                continue;
            };
            // Form bindings are never trusted, even when a draft submits a
            // nonempty spoof. An empty schema fingerprint marks the result
            // descriptor incomplete and lets the planner fill it in.
            let draft_descriptor = query.schema_fingerprint.is_empty();
            match &query.definition {
                ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } => {
                    let bindings = composition_inline_form_bindings_for_save(
                        &node.id,
                        sql,
                        &query.form_bindings,
                        previous_document.as_ref(),
                        &forms,
                    )?;
                    let metadata = self
                        .derive_inline_sql_query_metadata(
                            space_id,
                            &source_for_planning,
                            Some(&bindings),
                            &variables,
                            None,
                            None,
                            None,
                        )
                        .await?
                        .ok_or_else(|| {
                            AppError::invalid_input(
                                ErrorCode::InvalidInput,
                                "Composition SQL query cannot be resolved",
                            )
                        })?;
                    super::composition_resolver::reconcile_query_returns_with_schema(
                        returns,
                        &metadata.result_columns,
                        draft_descriptor,
                    )?;
                    query.form_bindings = metadata.form_bindings;
                    query.schema_fingerprint = metadata.schema_fingerprint;
                }
                ugoite_domain::composition::VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some(entry_id),
                    revision_id: Some(revision_id),
                    ..
                } => {
                    let metadata = self
                        .derive_saved_sql_query_metadata(
                            space_id,
                            &source_for_planning,
                            None,
                            None,
                            None,
                        )
                        .await?
                        .ok_or_else(|| {
                            AppError::not_found(ErrorCode::EntryNotFound, "Saved SQL not found")
                        })?;
                    if metadata.entry_id != *entry_id || metadata.revision_id != *revision_id {
                        return Err(AppError::not_found(
                            ErrorCode::EntryNotFound,
                            "Saved SQL revision not found",
                        )
                        .into());
                    }
                    super::composition_save_validation::validate_saved_sql_arguments(
                        &query.arguments,
                        &metadata.parameter_types,
                        &variables,
                    )?;
                    super::composition_resolver::reconcile_query_returns_with_schema(
                        returns,
                        &metadata.result_columns,
                        draft_descriptor,
                    )?;
                    query.form_bindings = metadata.form_bindings;
                    query.schema_fingerprint = metadata.schema_fingerprint;
                }
                ugoite_domain::composition::VariableGraphQueryDefinition::SavedSql { .. } => {
                    return Err(AppError::not_found(
                        ErrorCode::EntryNotFound,
                        "Saved SQL revision not found",
                    )
                    .into());
                }
            }
        }
        if !request.document.has_complete_query_descriptors() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition queries require current local result descriptors",
            )
            .into());
        }
        let canonical = ugoite_domain::composition::canonicalize_composition(&request.document)
            .map_err(|diagnostic| {
                AppError::invalid_input(ErrorCode::InvalidInput, diagnostic.as_str())
            })?;

        let exact_head_proof = self
            .preflight_composition_save_local(
                space_id,
                &canonical.yaml,
                &work.variables,
                &work.unset_variables,
            )
            .await?;

        let is_create = request.entry_id.is_none();
        let entry_id = request.entry_id.unwrap_or_else(|| {
            composition::composition_entry_id_for_operation(space_id, operation_id)
        });
        let entry_id_text = entry_id.to_string();
        validate_storage_id(validate_entry_id(&entry_id_text))?;
        if let Some(base_revision_id) = request.base_revision_id {
            validate_storage_id(validate_revision_id(&base_revision_id.to_string()))?;
        }

        let result = composition::save_composition_with_work_and_head_proof(
            &self.operator,
            space_id,
            &self.workspace_path(space_id),
            request,
            entry_id,
            canonical,
            author,
            operation_id,
            &request_yaml,
            &work,
            Some(&exact_head_proof),
        )
        .await?;
        let committed_revision_id = result.revision_id.to_string();
        self.record_committed_entry_revision_id(
            space_id,
            &entry_id_text,
            &committed_revision_id,
            if is_create {
                crate::mutation_audit::ENTRY_CREATED_ACTION
            } else {
                crate::mutation_audit::ENTRY_UPDATED_ACTION
            },
        )
        .await;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_exact_base_composition_is_not_treated_as_absent() {
        let malformed_yaml =
            BTreeMap::from([("spec".to_string(), Value::String("not: [valid".to_string()))]);
        let error = parse_exact_base_document(&malformed_yaml)
            .expect_err("malformed base YAML must fail closed");
        assert_eq!(
            error.downcast_ref::<AppError>().unwrap().code(),
            ErrorCode::InvalidInput
        );
        assert!(error.to_string().contains("invalid_composition"));

        let missing_spec = BTreeMap::new();
        let error = parse_exact_base_document(&missing_spec)
            .expect_err("missing base spec must fail closed");
        assert_eq!(
            error.downcast_ref::<AppError>().unwrap().code(),
            ErrorCode::InvalidInput
        );
    }
}
