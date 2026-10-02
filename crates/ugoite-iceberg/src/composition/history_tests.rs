use crate::{composition, iceberg_store, publication_context_for_change, service::UgoiteService};
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_domain::entry::{EntryOperation, EntryRevisionDraft};
use ugoite_domain::id::RevisionId;
use uuid::Uuid;

const MONTHLY_EXPENSE: &str =
    include_str!("../../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml");

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
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
                tags: None,
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
