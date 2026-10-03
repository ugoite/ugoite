use super::*;

impl UgoiteService {
    /// Restore one exact historical Composition revision for a trusted local
    /// Core caller. The restore is an append-only publication.
    pub async fn restore_composition_local(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        author: &str,
    ) -> Result<composition::CompositionRestoreResult> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(source_revision_id))?;

        let result = composition::restore_composition(
            &self.operator,
            &self.workspace_path(space_id),
            parse_entry_id(entry_id)?,
            parse_revision_id(source_revision_id)?,
            author,
        )
        .await?;
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
        )
        .await;
        Ok(result)
    }

    /// Restore one exact historical Composition revision after checking the
    /// current Entry and Form permissions under a held authorization lease.
    pub async fn restore_composition_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<composition::CompositionRestoreResult> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(source_revision_id))?;
        let parsed_entry_id = parse_entry_id(entry_id)?;

        let (state, authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        self.require_action_for_principals_in_state(
            &state,
            entry_id,
            ResourceKind::Entry,
            Action::Read,
            principal_ids,
        )?;
        self.require_action_for_principals_in_state(
            &state,
            entry_id,
            ResourceKind::Entry,
            Action::Update,
            principal_ids,
        )?;
        let form_resource = ResourceRef {
            kind: ResourceKind::Form,
            id: composition::COMPOSITION_REGISTRY_FORM_NAME.to_string(),
            parent: None,
        };
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, Some(&form_resource))?
                .contains(&Action::Read)
            {
                return Err(AppError::forbidden("Form is not readable").into());
            }
        }

        let scopes = self
            .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
            .await?;
        require_composition_entry_read_scope(&scopes, parsed_entry_id, entry_id)?;
        authorization_lease.prepare_mutation().await?;
        let result = crate::authorization::with_authorization_write_fence(
            authorization_lease.write_fence(),
            composition::restore_composition(
                &self.operator,
                &self.workspace_path(space_id),
                parsed_entry_id,
                parse_revision_id(source_revision_id)?,
                author,
            ),
        )
        .await?;
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
        )
        .await;
        Ok(result)
    }
}
