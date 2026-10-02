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

#[cfg(debug_assertions)]
#[path = "process_conflict_tests.rs"]
mod process_conflict_tests;

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

#[tokio::test]
async fn authorized_composition_source_descriptors_use_current_acl_and_exact_revision(
) -> anyhow::Result<()> {
    use ugoite_core::error::{AppError, ErrorCode};
    use ugoite_core::sql_query::SavedSqlRevisionRef;
    use ugoite_domain::id::FormId;

    let service = UgoiteService::new(format!(
        "memory://composition-source-descriptors-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_429_001);
    let viewer = Uuid::from_u128(3_429_002);
    let space_id = service
        .create_space_for_principal("composition-source-descriptors", owner, "Owner")
        .await?
        .to_string();
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

    let form_name = "Composition-Source";
    let form = service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "amount": {"id": 100, "type": "integer", "required": false}
                }
            }),
        )
        .await?;
    let form_id = form.form_id;
    let readable_form = service
        .get_composition_source_form_authorized_for_principals(&space_id, form_id, &[owner])
        .await?;
    assert_eq!(readable_form.id, form_id);
    assert_eq!(readable_form.fields[0].name, "amount");

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: form_name.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let denied_form = service
        .get_composition_source_form_authorized_for_principals(&space_id, form_id, &[viewer])
        .await
        .expect_err("denied Form schema must be concealed");
    let missing_form = service
        .get_composition_source_form_authorized_for_principals(
            &space_id,
            FormId::from(Uuid::now_v7()),
            &[viewer],
        )
        .await
        .expect_err("missing Form identity must be concealed");
    let denied_form = denied_form.downcast::<AppError>()?;
    let missing_form = missing_form.downcast::<AppError>()?;
    assert_eq!(denied_form.code(), ErrorCode::FormNotFound);
    assert_eq!(denied_form.code(), missing_form.code());
    assert_eq!(denied_form.message(), missing_form.message());

    let sql_id = format!("composition-source-{}", Uuid::now_v7());
    let payload = |name: &str, description: &str| crate::saved_sql::SqlPayload {
        name: Some("parameterized source".to_string()),
        kind: crate::saved_sql::SqlKind::UserQuery,
        metadata: None,
        sql: format!("SELECT ${name} AS result FROM \"{form_name}\""),
        variables: json!([{
            "name": name,
            "type": "string",
            "description": description
        }]),
    };
    let first = service
        .create_saved_sql_authorized_for_principals(
            &space_id,
            Some(&sql_id),
            &payload("month", "private first description"),
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let stored_first = service.get_saved_sql(&space_id, &sql_id).await?;
    assert_eq!(stored_first["metadata"]["bindingVersion"], json!(1));
    assert_eq!(
        stored_first["metadata"]["formBindings"],
        json!([{
            "name": form_name,
            "formId": form_id.to_string(),
        }]),
        "the exact Saved SQL revision fixture must contain server-derived Form bindings"
    );
    let first_revision_id = first["revision_id"]
        .as_str()
        .expect("created exact revision")
        .to_string();
    let second = service
        .update_saved_sql_authorized_for_principals(
            &space_id,
            &sql_id,
            &payload("year", "private second description"),
            &first_revision_id,
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let second_revision_id = second["revision_id"]
        .as_str()
        .expect("updated exact revision")
        .to_string();
    assert_ne!(first_revision_id, second_revision_id);

    let first_descriptor = service
        .get_saved_sql_revision_descriptor_authorized_for_principals(
            &space_id,
            &SavedSqlRevisionRef {
                id: sql_id.clone(),
                revision_id: first_revision_id.clone(),
            },
            &[owner],
        )
        .await?;
    assert_eq!(first_descriptor.id, sql_id);
    assert_eq!(first_descriptor.revision_id, first_revision_id);
    assert_eq!(first_descriptor.variables["month"].var_type, "string");
    let first_descriptor_debug = format!("{first_descriptor:?}");
    assert!(!first_descriptor_debug.contains("private first description"));
    assert!(!first_descriptor_debug.contains("SELECT"));
    assert!(!first_descriptor_debug.contains(form_name));
    assert!(!first_descriptor_debug.contains(&form_id.to_string()));

    let second_descriptor = service
        .get_saved_sql_revision_descriptor_authorized_for_principals(
            &space_id,
            &SavedSqlRevisionRef {
                id: sql_id.clone(),
                revision_id: second_revision_id.clone(),
            },
            &[owner],
        )
        .await?;
    assert_eq!(second_descriptor.variables["year"].var_type, "string");
    assert!(!second_descriptor.variables.contains_key("month"));

    let missing_sql = service
        .get_saved_sql_revision_descriptor_authorized_for_principals(
            &space_id,
            &SavedSqlRevisionRef {
                id: sql_id.clone(),
                revision_id: Uuid::now_v7().to_string(),
            },
            &[owner],
        )
        .await
        .expect_err("missing exact revision must not fall back to latest");
    assert_eq!(
        missing_sql.downcast::<AppError>()?.code(),
        ErrorCode::EntryNotFound
    );

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::SavedSql,
                id: sql_id.clone(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let denied_sql = service
        .get_saved_sql_revision_descriptor_authorized_for_principals(
            &space_id,
            &SavedSqlRevisionRef {
                id: sql_id.clone(),
                revision_id: first_revision_id,
            },
            &[viewer],
        )
        .await
        .expect_err("denied Saved SQL metadata must be concealed");
    let missing_sql = service
        .get_saved_sql_revision_descriptor_authorized_for_principals(
            &space_id,
            &SavedSqlRevisionRef {
                id: format!("missing-{}", Uuid::now_v7()),
                revision_id: Uuid::now_v7().to_string(),
            },
            &[viewer],
        )
        .await
        .expect_err("missing exact revision must be concealed");
    let denied_sql = denied_sql.downcast::<AppError>()?;
    let missing_sql = missing_sql.downcast::<AppError>()?;
    assert_eq!(denied_sql.code(), ErrorCode::EntryNotFound);
    assert_eq!(denied_sql.code(), missing_sql.code());
    assert_eq!(denied_sql.message(), missing_sql.message());
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
    let local_raw = service
        .get_composition_raw_local(&space_id, "composition-document")
        .await?;
    assert_eq!(local_raw.fields["format_version"], json!(99));
    assert_eq!(local_raw.fields["spec"], json!("not: [valid YAML"));
    assert_eq!(local_raw.revision.revision_id, raw.revision.revision_id);

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
    let local_exact = service
        .get_composition_raw_revision_local(&space_id, "composition-document", &first_revision_id)
        .await?;
    assert_eq!(local_exact.fields["spec"], json!("not: [valid YAML"));
    assert_eq!(local_exact.revision.revision_id, exact.revision.revision_id);
    let local_missing_revision = service
        .get_composition_raw_revision_local(
            &space_id,
            "composition-document",
            &Uuid::from_u128(3_428_099).to_string(),
        )
        .await
        .expect_err("local exact read must not fall back to latest");
    assert_eq!(
        local_missing_revision
            .downcast_ref::<ugoite_core::error::AppError>()
            .unwrap()
            .code(),
        ugoite_core::error::ErrorCode::EntryNotFound
    );

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
    let local_page = service
        .composition_history_local_page(&space_id, "composition-document", 1, 0)
        .await?;
    assert_eq!(local_page.total, 2);
    assert_eq!(local_page.limit, 1);
    assert_eq!(local_page.offset, 0);
    assert!(local_page.has_more);
    assert_eq!(
        local_page.revisions[0].revision.revision_id,
        exact.revision.revision_id
    );
    let local_second_page = service
        .composition_history_local_page(&space_id, "composition-document", 1, 1)
        .await?;
    assert_eq!(local_second_page.total, 2);
    assert!(!local_second_page.has_more);
    assert_eq!(local_second_page.revisions.len(), 1);
    assert_eq!(
        local_second_page.revisions[0].revision.revision_id,
        latest.revision.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn local_composition_raw_reads_do_not_create_registry() -> anyhow::Result<()> {
    use ugoite_core::error::{AppError, ErrorCode};

    let service = UgoiteService::new(format!(
        "memory://composition-local-missing-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_428_004);
    let space_id = service
        .create_space_for_principal("composition-local-missing", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);

    let missing_current = service
        .get_composition_raw_local(&space_id, "composition-not-created")
        .await
        .expect_err("missing local Composition should be not found");
    let missing_exact = service
        .get_composition_raw_revision_local(
            &space_id,
            "composition-not-created",
            &Uuid::now_v7().to_string(),
        )
        .await
        .expect_err("missing local exact revision should be not found");
    let missing_history = service
        .composition_history_local_page(&space_id, "composition-not-created", 10, 0)
        .await
        .expect_err("missing local Composition history should be not found");
    for error in [missing_current, missing_exact, missing_history] {
        assert_eq!(
            error.downcast::<AppError>()?.code(),
            ErrorCode::EntryNotFound
        );
    }

    let workspace =
        iceberg_store::native_workspace_read_only(service.operator(), &workspace_path).await?;
    let publication = workspace.current_publication().await?;
    let forms = workspace.forms_at_publication(&publication).await?;
    assert!(!forms.iter().any(|form| {
        form.name
            .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)
    }));
    Ok(())
}

#[tokio::test]
async fn local_composition_raw_reads_reject_invalid_ids_and_history_limits() -> anyhow::Result<()> {
    use ugoite_core::error::{AppError, ErrorCode};

    let service = UgoiteService::new(format!(
        "memory://composition-local-read-validation-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_428_005);
    let space_id = service
        .create_space_for_principal("composition-local-read-validation", owner, "Owner")
        .await?
        .to_string();
    let valid_entry_id = Uuid::now_v7().to_string();

    let invalid_current = service
        .get_composition_raw_local(&space_id, "bad/entry-id")
        .await
        .expect_err("current raw read must reject a malformed Entry ID");
    let invalid_exact_entry = service
        .get_composition_raw_revision_local(&space_id, "bad/entry-id", &Uuid::now_v7().to_string())
        .await
        .expect_err("exact raw read must reject a malformed Entry ID");
    let invalid_exact_revision = service
        .get_composition_raw_revision_local(&space_id, &valid_entry_id, "bad/revision-id")
        .await
        .expect_err("exact raw read must reject a malformed Revision ID");
    let invalid_history_entry = service
        .composition_history_local_page(&space_id, "bad/entry-id", 10, 0)
        .await
        .expect_err("history read must reject a malformed Entry ID");

    for error in [
        invalid_current,
        invalid_exact_entry,
        invalid_exact_revision,
        invalid_history_entry,
    ] {
        assert_eq!(
            error.downcast::<AppError>()?.code(),
            ErrorCode::InvalidIdentifier
        );
    }

    let zero_limit = service
        .composition_history_local_page(&space_id, &valid_entry_id, 0, 0)
        .await
        .expect_err("history page size zero must be rejected");
    let over_limit = service
        .composition_history_local_page(
            &space_id,
            &valid_entry_id,
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE + 1,
            0,
        )
        .await
        .expect_err("history page size above the maximum must be rejected");

    for error in [zero_limit, over_limit] {
        assert_eq!(
            error.downcast::<AppError>()?.code(),
            ErrorCode::InvalidInput
        );
    }
    Ok(())
}
