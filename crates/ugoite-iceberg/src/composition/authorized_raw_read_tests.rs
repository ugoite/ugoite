use crate::authorization::{Authorizer, ResourceKind, ResourceRef};
use crate::{composition, iceberg_store, publication_context_for_change, service::UgoiteService};
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::FormVersion;
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use uuid::Uuid;

#[tokio::test]
async fn authorized_composition_raw_read_conceals_missing_and_denied_ids() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!("memory://composition-acl-{}", Uuid::now_v7()))?;
    let owner = Uuid::from_u128(3_428_001);
    let viewer = Uuid::from_u128(3_428_002);
    let space_id = service
        .create_space_for_principal("composition-acl", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);
    let registry =
        composition::ensure_composition_registry(service.operator(), &workspace_path).await?;

    let entry_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"composition-denied");
    let revision_id = Uuid::now_v7();
    let committed_at_micros = 1_800_000_000_000_000;
    let author = owner.to_string();
    let change_id = Uuid::now_v7().to_string();
    let revision = EntryRevision {
        form_id: registry.id,
        entry_id: EntryId::from(entry_id),
        revision_id: RevisionId::from(revision_id),
        change_id: change_id.clone(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros,
        author_id: author.clone(),
        form_version: registry.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: "composition-denied".to_string(),
            created_at_micros: committed_at_micros,
            updated_at_micros: committed_at_micros,
            updated_by: author.clone(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([
            (FieldId::new(100)?, FieldValue::String("Tool".into())),
            (FieldId::new(101)?, FieldValue::String("dashboard".into())),
            (FieldId::new(102)?, FieldValue::Integer(1)),
            (FieldId::new(103)?, FieldValue::String("name: tool".into())),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author,
        message: Some("seed a Composition revision for ACL read coverage".into()),
        reverts_change_id: None,
        created_at_micros: committed_at_micros,
    };
    let workspace = iceberg_store::native_workspace(service.operator(), &workspace_path).await?;
    workspace
        .commit(publication_context_for_change(
            &change,
            "test.composition.acl-seed",
            &revision,
        )?)?
        .append_composition_revisions_authorized(registry.id, vec![revision], None)
        .await?;
    let revision_id = revision_id.to_string();

    let authorizer = Authorizer::new(service.operator().clone());
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_string(),
                state: PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "composition-denied".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;

    let visible = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-denied", &[owner])
        .await?;
    assert_eq!(visible.fields["name"], json!("Tool"));
    let exact = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            "composition-denied",
            &revision_id,
            &[owner],
        )
        .await?;
    assert_eq!(exact.revision.revision_id.to_string(), revision_id);
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "composition-denied",
            &[owner],
            10,
            0,
        )
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(
        history.revisions[0].revision.revision_id.to_string(),
        revision_id
    );

    let denied = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-denied", &[viewer])
        .await
        .expect_err("Entry-level deny must hide the Composition");
    let missing = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-missing", &[viewer])
        .await
        .expect_err("missing Composition must remain concealed");
    let denied = denied.downcast::<ugoite_core::error::AppError>()?;
    let missing = missing.downcast::<ugoite_core::error::AppError>()?;
    assert_eq!(denied.code(), ugoite_core::error::ErrorCode::EntryNotFound);
    assert_eq!(denied.code(), missing.code());
    assert!(denied.message().starts_with("Entry not found:"));
    assert!(missing.message().starts_with("Entry not found:"));

    let denied_invalid_page = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "composition-denied",
            &[viewer],
            0,
            0,
        )
        .await
        .expect_err("invalid pagination is rejected before ACL-dependent reads");
    let missing_invalid_page = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "composition-missing",
            &[viewer],
            0,
            0,
        )
        .await
        .expect_err("invalid pagination is rejected before existence checks");
    let denied_invalid_page = denied_invalid_page.downcast::<ugoite_core::error::AppError>()?;
    let missing_invalid_page = missing_invalid_page.downcast::<ugoite_core::error::AppError>()?;
    assert_eq!(
        denied_invalid_page.code(),
        ugoite_core::error::ErrorCode::InvalidInput
    );
    assert_eq!(denied_invalid_page.code(), missing_invalid_page.code());
    assert_eq!(
        denied_invalid_page.message(),
        missing_invalid_page.message()
    );
    Ok(())
}

struct RawCompositionRevisionArgs<'a> {
    form_id: FormId,
    form_version: FormVersion,
    entry_id: Uuid,
    revision_id: Uuid,
    parent_revision_id: Option<RevisionId>,
    entry_version: u64,
    expected_version: Option<u64>,
    change_id: String,
    author: String,
    committed_at_micros: i64,
    external_id: &'a str,
    tags: Vec<String>,
    name: &'a str,
    format_version: i64,
    spec: &'a str,
}

fn raw_composition_revision(args: RawCompositionRevisionArgs<'_>) -> anyhow::Result<EntryRevision> {
    let RawCompositionRevisionArgs {
        form_id,
        form_version,
        entry_id,
        revision_id,
        parent_revision_id,
        entry_version,
        expected_version,
        change_id,
        author,
        committed_at_micros,
        external_id,
        tags,
        name,
        format_version,
        spec,
    } = args;
    Ok(EntryRevision {
        form_id,
        entry_id: EntryId::from(entry_id),
        revision_id: RevisionId::from(revision_id),
        change_id,
        parent_revision_id,
        entry_version,
        expected_version,
        operation: EntryOperation::Upsert,
        committed_at_micros,
        author_id: author.clone(),
        form_version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            external_id: external_id.to_string(),
            tags,
            created_at_micros: committed_at_micros,
            updated_at_micros: committed_at_micros,
            updated_by: author,
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([
            (FieldId::new(100)?, FieldValue::String(name.to_string())),
            (FieldId::new(101)?, FieldValue::String("dashboard".into())),
            (FieldId::new(102)?, FieldValue::Integer(format_version)),
            (FieldId::new(103)?, FieldValue::String(spec.to_string())),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    })
}

#[tokio::test]
async fn raw_composition_read_preserves_unknown_version_and_exact_history() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!("memory://composition-raw-{}", Uuid::now_v7()))?;
    let owner = Uuid::from_u128(3_428_003);
    let space_id = service
        .create_space_for_principal("composition-raw", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);
    let registry =
        composition::ensure_composition_registry(service.operator(), &workspace_path).await?;
    let workspace = iceberg_store::native_workspace(service.operator(), &workspace_path).await?;
    let missing_history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "missing-composition",
            &[owner],
            10,
            0,
        )
        .await
        .expect_err("missing Composition history should be not found");
    assert_eq!(
        missing_history
            .downcast_ref::<ugoite_core::error::AppError>()
            .unwrap()
            .code(),
        ugoite_core::error::ErrorCode::EntryNotFound
    );

    let entry_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"composition-document");
    let author = owner.to_string();
    let created_at_micros = 1_800_000_000_000_000;
    let first_revision_id = Uuid::now_v7();
    let first_change_id = Uuid::now_v7().to_string();
    let first_revision = raw_composition_revision(RawCompositionRevisionArgs {
        form_id: registry.id,
        form_version: registry.version,
        entry_id,
        revision_id: first_revision_id,
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        change_id: first_change_id.clone(),
        author: author.clone(),
        committed_at_micros: created_at_micros,
        external_id: "composition-document",
        tags: vec!["tool".to_string()],
        name: "Raw tool",
        format_version: 99,
        spec: "not: [valid YAML",
    })?;
    let first_change = ChangeCommand {
        change_id: first_change_id,
        run_id: None,
        actor_principal_id: author.clone(),
        message: Some("seed an unknown-version raw Composition revision".into()),
        reverts_change_id: None,
        created_at_micros,
    };
    workspace
        .commit(publication_context_for_change(
            &first_change,
            "test.composition.raw-seed",
            &first_revision,
        )?)?
        .append_composition_revisions_authorized(registry.id, vec![first_revision], None)
        .await?;
    let first_revision_id = first_revision_id.to_string();

    let raw = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-document", &[owner])
        .await?;
    assert_eq!(raw.fields["format_version"], json!(99));
    assert_eq!(raw.format_version_probe(), Some(99));
    assert_eq!(raw.fields["spec"], json!("not: [valid YAML"));
    assert_eq!(raw.revision.entry.tags, ["tool"]);

    let second_revision_id = Uuid::now_v7();
    let second_change_id = Uuid::now_v7().to_string();
    let second_revision = raw_composition_revision(RawCompositionRevisionArgs {
        form_id: registry.id,
        form_version: registry.version,
        entry_id,
        revision_id: second_revision_id,
        parent_revision_id: Some(RevisionId::from(Uuid::parse_str(&first_revision_id)?)),
        entry_version: 2,
        expected_version: Some(1),
        change_id: second_change_id.clone(),
        author: author.clone(),
        committed_at_micros: created_at_micros + 1,
        external_id: "composition-document",
        tags: vec!["updated".to_string()],
        name: "Updated tool",
        format_version: 1,
        spec: "name: updated",
    })?;
    let second_change = ChangeCommand {
        change_id: second_change_id,
        run_id: None,
        actor_principal_id: author,
        message: Some("append a newer raw Composition revision".into()),
        reverts_change_id: None,
        created_at_micros: created_at_micros + 1,
    };
    workspace
        .commit(publication_context_for_change(
            &second_change,
            "test.composition.raw-update",
            &second_revision,
        )?)?
        .append_composition_revisions_authorized(registry.id, vec![second_revision], None)
        .await?;

    let exact = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            "composition-document",
            &first_revision_id,
            &[owner],
        )
        .await?;
    assert_eq!(exact.fields["spec"], json!("not: [valid YAML"));
    let missing_revision = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            "composition-document",
            &Uuid::from_u128(3_428_099).to_string(),
            &[owner],
        )
        .await
        .expect_err("exact read must not fall back to latest");
    assert_eq!(
        missing_revision
            .downcast_ref::<ugoite_core::error::AppError>()
            .unwrap()
            .code(),
        ugoite_core::error::ErrorCode::EntryNotFound
    );
    let latest = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-document", &[owner])
        .await?;
    assert_eq!(latest.fields["spec"], json!("name: updated"));
    assert_ne!(latest.revision.revision_id, exact.revision.revision_id);

    let page = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "composition-document",
            &[owner],
            1,
            0,
        )
        .await?;
    assert_eq!(page.total, 2);
    assert!(page.has_more);
    assert_eq!(
        page.revisions[0].revision.revision_id,
        exact.revision.revision_id
    );
    Ok(())
}
