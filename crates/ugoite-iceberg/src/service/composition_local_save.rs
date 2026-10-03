use super::*;

impl UgoiteService {
    /// Create or update one Composition through the principal-free local Core
    /// path. Remote adapters must use the authorized save method.
    pub async fn save_composition_local(
        &self,
        space_id: &str,
        request: composition::CompositionSaveRequest,
        author: &str,
    ) -> Result<composition::CompositionSaveResult> {
        if request.entry_id.is_some() != request.base_revision_id.is_some() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "Composition updates require an entry ID and exact base revision",
            )
            .into());
        }

        // Reject unsupported or invalid documents before registry creation or
        // any other persistent side effect.
        let canonical = ugoite_domain::composition::canonicalize_composition(&request.document)
            .map_err(|diagnostic| {
                AppError::invalid_input(ErrorCode::InvalidInput, diagnostic.as_str())
            })?;
        let operation_id = Uuid::now_v7().to_string();
        composition::validate_operation_id(&operation_id)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;

        let is_create = request.entry_id.is_none();
        let entry_id = request.entry_id.unwrap_or_else(|| {
            composition::composition_entry_id_for_operation(space_id, &operation_id)
        });
        let entry_id_text = entry_id.to_string();
        validate_storage_id(validate_entry_id(&entry_id_text))?;
        if let Some(base_revision_id) = request.base_revision_id {
            validate_storage_id(validate_revision_id(&base_revision_id.to_string()))?;
        }

        let result = composition::save_composition(
            &self.operator,
            space_id,
            &self.workspace_path(space_id),
            request,
            entry_id,
            canonical,
            author,
            &operation_id,
        )
        .await?;
        self.record_committed_entry_revision(
            space_id,
            &entry_id_text,
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
