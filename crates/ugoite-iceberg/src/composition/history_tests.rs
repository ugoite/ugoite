use crate::{composition, iceberg_store, publication_context_for_change, service::UgoiteService};
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_domain::composition::{
    VariableGraphNode, VariableGraphNodeKind, VariableGraphScalarDescriptor,
    VariableGraphScalarType,
};
use ugoite_domain::entry::{EntryOperation, EntryRevisionDraft};
use ugoite_domain::id::RevisionId;
use uuid::Uuid;

const SQL_FIRST_COMPOSITION: &str =
    include_str!("../../tests/fixtures/composition-sql-first.ugcomp.yaml");

#[tokio::test]
async fn invalid_scalar_first_save_does_not_publish_registry_or_entry() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-invalid-first-save-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-invalid-first-save", owner, "Owner")
        .await?
        .to_string();
    let changes_before = service.list_changes(&space_id).await?;
    for (invalid_sql, fingerprinted_descriptor) in [
        ("SELECT FROM", false),
        ("SELECT 1 AS total WHERE false", false),
        ("SELECT 1 AS total UNION ALL SELECT 2 AS total", false),
        ("SELECT 'wrong-type' AS total", true),
    ] {
        let mut document = parse_composition_yaml(SQL_FIRST_COMPOSITION)
            .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
        let ugoite_domain::composition::VariableGraphNodeKind::Query { query, .. } =
            &mut document.spec.variables[0].kind
        else {
            anyhow::bail!("expected scalar query variable")
        };
        let ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } =
            &mut query.definition
        else {
            anyhow::bail!("expected inline SQL")
        };
        *sql = invalid_sql.to_owned();
        if fingerprinted_descriptor {
            query.schema_fingerprint = "previously-resolved-schema".to_owned();
        }
        let result = service
            .save_composition_authorized_for_principals(
                &space_id,
                composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document,
                },
                &owner.to_string(),
                &[owner],
            )
            .await;
        assert!(result.is_err(), "invalid scalar accepted: {invalid_sql}");
        let forms = iceberg_store::native_workspace_read_only(
            service.operator(),
            &service.workspace_path(&space_id),
        )
        .await?
        .list_forms()
        .await?;
        assert!(
            forms.iter().all(|form| !form
                .name
                .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)),
            "failed first save published Registry for {invalid_sql}"
        );
        assert_eq!(service.list_changes(&space_id).await?, changes_before);
    }

    // A rejected first save leaves the author able to correct the document
    // and publish one ordinary revision afterward.
    let repaired = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: parse_composition_yaml(SQL_FIRST_COMPOSITION)
                    .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &repaired.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(
        history.revisions[0].revision.revision_id,
        repaired.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn invalid_scalar_update_preserves_history_until_repaired_save() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-invalid-scalar-update-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-invalid-scalar-update", owner, "Owner")
        .await?
        .to_string();
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: parse_composition_yaml(SQL_FIRST_COMPOSITION)
                    .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let mut invalid = saved.document.clone();
    let VariableGraphNodeKind::Query { query, .. } = &mut invalid.spec.variables[0].kind else {
        anyhow::bail!("expected scalar query variable")
    };
    let ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } =
        &mut query.definition
    else {
        anyhow::bail!("expected inline SQL")
    };
    *sql = "SELECT 1 AS total UNION ALL SELECT 2 AS total".to_owned();

    let rejected = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(saved.entry_id.clone()),
                base_revision_id: Some(saved.revision_id.clone()),
                document: invalid,
            },
            &owner.to_string(),
            &[owner],
        )
        .await;
    assert!(rejected.is_err(), "multirow scalar update must be rejected");

    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &saved.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(
        history.total, 1,
        "a rejected update must append no revision"
    );
    assert_eq!(history.revisions[0].revision.revision_id, saved.revision_id);

    let mut repaired = saved.document;
    let VariableGraphNodeKind::Query { query, .. } = &mut repaired.spec.variables[0].kind else {
        anyhow::bail!("expected scalar query variable")
    };
    let ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } =
        &mut query.definition
    else {
        anyhow::bail!("expected inline SQL")
    };
    *sql = "SELECT 2 AS total".to_owned();
    let repaired_save = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(saved.entry_id.clone()),
                base_revision_id: Some(saved.revision_id),
                document: repaired,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &saved.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 2, "repair must append one revision");
    assert_eq!(
        history.revisions[1].revision.revision_id,
        repaired_save.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn nullable_sql_null_first_save_is_valid_and_resolves_nullable_descriptor(
) -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-nullable-first-save-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-nullable-first-save", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(SQL_FIRST_COMPOSITION)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let VariableGraphNodeKind::Query { query, returns } = &mut document.spec.variables[0].kind
    else {
        anyhow::bail!("expected scalar query variable")
    };
    if let ugoite_domain::composition::VariableGraphResultType::Scalar { value_type } = returns {
        value_type.nullable = true;
    }
    let ugoite_domain::composition::VariableGraphQueryDefinition::InlineSql { sql } =
        &mut query.definition
    else {
        anyhow::bail!("expected inline SQL")
    };
    *sql = "SELECT CAST(NULL AS BIGINT) AS total".to_owned();
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let VariableGraphNodeKind::Query { returns, .. } = &saved.document.spec.variables[0].kind
    else {
        anyhow::bail!("saved scalar query disappeared")
    };
    assert!(matches!(
        returns,
        ugoite_domain::composition::VariableGraphResultType::Scalar { value_type }
            if value_type.nullable
    ));
    Ok(())
}

#[tokio::test]
async fn missing_or_mistyped_typed_work_rejects_first_save_without_registry() -> anyhow::Result<()>
{
    let service = UgoiteService::new(format!(
        "memory://composition-invalid-work-first-save-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-invalid-work-first-save", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(SQL_FIRST_COMPOSITION)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.spec.variables.push(VariableGraphNode {
        id: "required_input".to_owned(),
        kind: VariableGraphNodeKind::Input {
            value_type: VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Date,
                nullable: false,
            },
            required: true,
            default: None,
        },
    });
    for variables in [
        BTreeMap::new(),
        BTreeMap::from([("required_input".to_owned(), json!(42))]),
    ] {
        let result = service
            .save_composition_authorized_for_principals_with_work(
                &space_id,
                composition::CompositionSaveRequest {
                    entry_id: None,
                    base_revision_id: None,
                    document: document.clone(),
                },
                &owner.to_string(),
                &[owner],
                "invalid-work-first-save",
                composition::CompositionSaveWork {
                    variables,
                    unset_variables: Vec::new(),
                },
            )
            .await;
        assert!(result.is_err(), "incomplete or mistyped Work was accepted");
        let forms = iceberg_store::native_workspace_read_only(
            service.operator(),
            &service.workspace_path(&space_id),
        )
        .await?
        .list_forms()
        .await?;
        assert!(forms.iter().all(|form| {
            !form
                .name
                .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)
        }));
    }
    Ok(())
}

#[tokio::test]
async fn unauthorized_first_save_does_not_initialize_registry() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-unauthorized-first-save-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-unauthorized-first-save", owner, "Owner")
        .await?
        .to_string();
    let stranger = Uuid::now_v7();
    let result = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: parse_composition_yaml(SQL_FIRST_COMPOSITION)
                    .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?,
            },
            &stranger.to_string(),
            &[stranger],
        )
        .await;
    assert!(
        result.is_err(),
        "unauthorized principal created Composition"
    );
    let forms = iceberg_store::native_workspace_read_only(
        service.operator(),
        &service.workspace_path(&space_id),
    )
    .await?
    .list_forms()
    .await?;
    assert!(forms.iter().all(|form| {
        !form
            .name
            .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)
    }));
    Ok(())
}

#[tokio::test]
async fn successful_save_rejects_operation_replay_with_changed_work() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-changed-work-replay-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-changed-work-replay", owner, "Owner")
        .await?
        .to_string();
    let request = composition::CompositionSaveRequest {
        entry_id: None,
        base_revision_id: None,
        document: parse_composition_yaml(SQL_FIRST_COMPOSITION)
            .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?,
    };
    service
        .save_composition_authorized_for_principals_with_work(
            &space_id,
            request.clone(),
            &owner.to_string(),
            &[owner],
            "same-operation-changed-work",
            composition::CompositionSaveWork::default(),
        )
        .await?;
    let error = service
        .save_composition_authorized_for_principals_with_work(
            &space_id,
            request,
            &owner.to_string(),
            &[owner],
            "same-operation-changed-work",
            composition::CompositionSaveWork {
                variables: BTreeMap::from([("changed".to_owned(), json!(true))]),
                unset_variables: Vec::new(),
            },
        )
        .await
        .expect_err("same operation ID must reject changed Work");
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<AppError>()
            .is_some_and(|app_error| app_error.code() == ErrorCode::IdempotencyConflict)
    }));
    Ok(())
}

#[tokio::test]
async fn authorized_composition_history_pages_and_exact_reads_reach_revisions_after_read_cap(
) -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-history-pages-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-history-pages", owner, "Owner")
        .await?
        .to_string();
    let document = parse_composition_yaml(SQL_FIRST_COMPOSITION)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let workspace_path = service.workspace_path(&space_id);
    let registry =
        composition::ensure_composition_registry(service.operator(), &workspace_path).await?;
    let mut latest = service
        .get_composition_raw_local(&space_id, &saved.entry_id.to_string())
        .await?
        .revision;
    let change_id = Uuid::now_v7().to_string();
    let author = owner.to_string();
    let mut revisions = Vec::with_capacity(crate::MAX_NORMAL_READ_ROWS);

    for _ in 0..crate::MAX_NORMAL_READ_ROWS {
        let committed_at_micros = latest
            .committed_at_micros
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("test timestamp overflow"))?;
        let mut entry = latest.entry.clone();
        entry.updated_at_micros = committed_at_micros;
        entry.updated_by = author.clone();
        let next = EntryRevisionDraft {
            form_id: registry.id,
            entry_id: saved.entry_id,
            revision_id: RevisionId::from(Uuid::now_v7()),
            change_id: change_id.clone(),
            operation: EntryOperation::Upsert,
            committed_at_micros,
            author_id: author.clone(),
            form_version: registry.version,
            source_kind: "test".to_string(),
            source_id: None,
            entry,
            values: latest.values.clone(),
            extra_attributes: latest.extra_attributes.clone(),
            extension_metadata: latest.extension_metadata.clone(),
        }
        .build(&registry, Some(&latest))?;
        latest = next.clone();
        revisions.push(next);
    }

    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author,
        message: Some("seed an extended Composition history".to_string()),
        reverts_change_id: None,
        created_at_micros: revisions[0].committed_at_micros,
    };
    let context =
        publication_context_for_change(&change, "test.composition.extended-history", &revisions)?;
    iceberg_store::native_workspace(service.operator(), &workspace_path)
        .await?
        .commit(context)?
        .append_composition_revisions_authorized(registry.id, revisions.clone(), None)
        .await?;

    let entry_id = saved.entry_id.to_string();
    let second_to_last = revisions[revisions.len() - 2].revision_id;
    let last = revisions[revisions.len() - 1].revision_id;
    let page = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &entry_id,
            &[owner],
            1,
            crate::MAX_NORMAL_READ_ROWS - 1,
        )
        .await?;
    assert_eq!(page.total, crate::MAX_NORMAL_READ_ROWS + 1);
    assert_eq!(page.offset, crate::MAX_NORMAL_READ_ROWS - 1);
    assert!(page.has_more);
    assert_eq!(page.revisions.len(), 1);
    assert_eq!(page.revisions[0].revision.revision_id, second_to_last);

    let last_page = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &entry_id,
            &[owner],
            1,
            crate::MAX_NORMAL_READ_ROWS,
        )
        .await?;
    assert_eq!(last_page.total, page.total);
    assert!(!last_page.has_more);
    assert_eq!(last_page.revisions[0].revision.revision_id, last);

    let beyond_history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &entry_id,
            &[owner],
            1,
            usize::MAX,
        )
        .await?;
    assert_eq!(beyond_history.total, page.total);
    assert!(beyond_history.revisions.is_empty());
    assert!(!beyond_history.has_more);

    let exact = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            &entry_id,
            &second_to_last.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(exact.revision.revision_id, second_to_last);
    assert_ne!(exact.revision.revision_id, last);

    let exact_last = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            &entry_id,
            &last.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(exact_last.revision.revision_id, last);
    assert_ne!(exact_last.revision.revision_id, second_to_last);
    Ok(())
}
