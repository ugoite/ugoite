use super::*;

impl UgoiteService {
    /// Restore one exact historical Composition revision for a trusted local
    /// Core caller. The restore is an append-only publication. This
    /// convenience method creates a fresh operation ID, so it is for one-shot
    /// calls; callers that may retry an uncertain outcome must retain and use
    /// [`Self::restore_composition_local_with_operation_id`] instead.
    pub async fn restore_composition_local(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        author: &str,
    ) -> Result<composition::CompositionRestoreResult> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let current = composition::read_composition_raw_latest_including_tombstones(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
        )
        .await?
        .ok_or_else(|| super::composition_entry_not_found(entry_id))?;
        let base_revision_id = current.revision.revision_id.to_string();
        let operation_id = Uuid::now_v7().to_string();
        self.restore_composition_local_with_operation_id(
            space_id,
            entry_id,
            source_revision_id,
            &base_revision_id,
            author,
            &operation_id,
        )
        .await
    }

    /// Restore one exact historical revision with a caller-stable local
    /// operation identity and exact current base revision. Reuse the same ID
    /// and publication-defining arguments to recover an uncertain outcome.
    /// Within a Space, the ID cannot be reused for a different Entry, source
    /// revision, base revision, or author.
    pub async fn restore_composition_local_with_operation_id(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        base_revision_id: &str,
        author: &str,
        operation_id: &str,
    ) -> Result<composition::CompositionRestoreResult> {
        composition::validate_operation_id(operation_id)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(source_revision_id))?;
        validate_storage_id(validate_revision_id(base_revision_id))?;

        let result = composition::restore_composition(
            &self.operator,
            space_id,
            &self.workspace_path(space_id),
            parse_entry_id(entry_id)?,
            parse_revision_id(source_revision_id)?,
            parse_revision_id(base_revision_id)?,
            author,
            operation_id,
        )
        .await?;
        self.record_committed_entry_revision_id(
            space_id,
            entry_id,
            &result.revision_id.to_string(),
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
        )
        .await;
        Ok(result)
    }

    /// Restore one exact historical Composition revision after checking the
    /// current Entry and Form permissions under a held authorization lease.
    /// This convenience method creates a fresh operation ID, so it is for
    /// one-shot calls; callers that may retry an uncertain outcome must retain
    /// and use
    /// [`Self::restore_composition_authorized_for_principals_with_operation_id`]
    /// instead.
    pub async fn restore_composition_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<composition::CompositionRestoreResult> {
        let current = self
            .get_composition_raw_latest_authorized_for_principals_including_tombstones(
                space_id,
                entry_id,
                principal_ids,
            )
            .await?;
        let base_revision_id = current.revision.revision_id.to_string();
        let operation_id = Uuid::now_v7().to_string();
        self.restore_composition_authorized_for_principals_with_operation_id(
            space_id,
            entry_id,
            source_revision_id,
            &base_revision_id,
            author,
            principal_ids,
            &operation_id,
        )
        .await
    }

    async fn get_composition_raw_latest_authorized_for_principals_including_tombstones(
        &self,
        space_id: &str,
        entry_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<composition::RawCompositionRevision> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let parsed_entry_id = parse_entry_id(entry_id)?;
        Authorizer::new(self.operator.clone())
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                require_composition_entry_read_scope(&scopes, parsed_entry_id, entry_id)?;
                composition::read_composition_raw_latest_including_tombstones(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                )
                .await?
                .ok_or_else(|| super::composition_entry_not_found(entry_id))
            })
            .await
    }

    /// Restore one exact historical revision through current Composition and
    /// Form authorization, resolving retries from the durable publication.
    /// Reuse the same ID and publication-defining arguments to recover an
    /// uncertain outcome. Within a Space, the ID cannot be reused for a
    /// different Entry, source revision, base revision, or author. Current
    /// authorization is checked on every call, including retries, before the
    /// durable publication is resolved; an authorization denial can therefore
    /// precede an idempotency conflict.
    #[allow(clippy::too_many_arguments)]
    pub async fn restore_composition_authorized_for_principals_with_operation_id(
        &self,
        space_id: &str,
        entry_id: &str,
        source_revision_id: &str,
        base_revision_id: &str,
        author: &str,
        principal_ids: &[Uuid],
        operation_id: &str,
    ) -> Result<composition::CompositionRestoreResult> {
        require_nonempty_authorized_principals(principal_ids)?;
        composition::validate_operation_id(operation_id)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(source_revision_id))?;
        validate_storage_id(validate_revision_id(base_revision_id))?;
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
                space_id,
                &self.workspace_path(space_id),
                parsed_entry_id,
                parse_revision_id(source_revision_id)?,
                parse_revision_id(base_revision_id)?,
                author,
                operation_id,
            ),
        )
        .await?;
        self.record_committed_entry_revision_id(
            space_id,
            entry_id,
            &result.revision_id.to_string(),
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
        )
        .await;
        Ok(result)
    }
}
