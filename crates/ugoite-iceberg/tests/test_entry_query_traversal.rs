//! PR5 (Follow-up G) EntryQuery traversal and checkpoint regression coverage.
//!
//! The canonical EntryQuery compiler pages by keyset over an immutable
//! publication: the first page pins the publication, the signed cursor carries
//! the sort tuple, and continuation re-executes at the pinned checkpoint.
//! These tests pin that contract with short cursor pages and small fixtures:
//!
//! - repeated sort keys stay duplicate/skip-free via the hidden stable identity;
//! - null sort values order across cursor continuation (`NULLS LAST`);
//! - multi-column sorts (`Status ASC`, `updated_at DESC`, hidden entry id)
//!   survive page boundaries;
//! - live create/update/delete between pages does not move the pinned read;
//! - an authorization change after cursor issuance rejects the continuation;
//! - a deterministic traversal includes every row, even on a partial final page.
//!
//! No offset-paging equivalence is asserted anywhere; there are no compat
//! adapters in this file.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Value};
use ugoite_core::entry_query::{
    EntryFieldRef, EntryPageRequest, EntryProjection, EntryQuery, EntryQueryScope, EntrySort,
    EntrySortDirection,
};
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

async fn setup_space(uri: &str, slug: &str) -> Result<(UgoiteService, String)> {
    let service = UgoiteService::new(uri)?;
    service.create_space(slug).await?;
    Ok((service, slug.to_string()))
}

async fn upsert_task_form(service: &UgoiteService, space_id: &str) -> Result<()> {
    service
        .upsert_form(
            space_id,
            &json!({
                "name": "Task",
                "fields": {
                    "Status": {"type": "string"},
                    "Priority": {"type": "long"},
                    "Nickname": {"type": "string"},
                },
            }),
        )
        .await?;
    Ok(())
}

async fn form_id(
    service: &UgoiteService,
    space_id: &str,
    form: &str,
) -> Result<ugoite_domain::id::FormId> {
    let form_json = service.get_form(space_id, form).await?;
    serde_json::from_value(form_json["id"].clone()).context("test Form is missing its id")
}

async fn property_ref(
    service: &UgoiteService,
    space_id: &str,
    form: &str,
    field: &str,
) -> Result<EntryFieldRef> {
    let form_json = service.get_form(space_id, form).await?;
    let capability = form_json
        .pointer(&format!("/fields/{field}/query_capability/field"))
        .cloned()
        .with_context(|| format!("test Form field {field} is missing its query capability"))?;
    serde_json::from_value(capability).context("test field capability is not an EntryFieldRef")
}

async fn create_task(
    service: &UgoiteService,
    space_id: &str,
    entry_id: &str,
    fields: BTreeMap<String, Value>,
) -> Result<()> {
    service
        .create_structured_entry_with_receipt(
            space_id,
            entry_id,
            "Task".to_string(),
            Vec::new(),
            fields,
            BTreeMap::new(),
            "owner",
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn text_search_ignores_uuid_and_matches_scalar_lists() -> Result<()> {
    let (service, space_id) =
        setup_space("memory://entry-query-text-typed-fields", "typed-search").await?;
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "Task",
                "fields": {
                    "Reference": {"type": "uuid"},
                    "Labels": {"type": "list", "items": {"type": "string"}},
                },
            }),
        )
        .await?;

    service
        .create_structured_entry_with_receipt(
            &space_id,
            "list-match",
            "Task".to_string(),
            Vec::new(),
            BTreeMap::from([
                (
                    "Reference".to_string(),
                    json!("ffffffff-ffff-ffff-ffff-ffffffffffff"),
                ),
                ("Labels".to_string(), json!(["priority needle"])),
            ]),
            BTreeMap::new(),
            "owner",
        )
        .await?;

    let page = service
        .query_entry_page(
            &space_id,
            page_request(
                EntryQuery {
                    scope: EntryQueryScope::Form {
                        form_id: form_id(&service, &space_id, "Task").await?,
                    },
                    text: Some("needle".to_string()),
                    filters: Vec::new(),
                    sort: Vec::new(),
                },
                10,
                None,
            ),
        )
        .await?;

    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["list-match"]
    );
    Ok(())
}

fn task_fields(
    status: Option<&str>,
    priority: Option<i64>,
    nickname: Option<&str>,
) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    if let Some(status) = status {
        fields.insert("Status".to_string(), Value::String(status.to_string()));
    }
    if let Some(priority) = priority {
        fields.insert("Priority".to_string(), json!(priority));
    }
    if let Some(nickname) = nickname {
        fields.insert("Nickname".to_string(), Value::String(nickname.to_string()));
    }
    fields
}

fn page_request(query: EntryQuery, limit: usize, after: Option<String>) -> EntryPageRequest {
    EntryPageRequest {
        query,
        projection: EntryProjection::Preview,
        limit,
        after,
    }
}

/// Drains one query through the cursor chain and returns every row id in order.
async fn collect_ids(
    service: &UgoiteService,
    space_id: &str,
    query: &EntryQuery,
    page_limit: usize,
) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut after = None;
    loop {
        let page = service
            .query_entry_page(space_id, page_request(query.clone(), page_limit, after))
            .await?;
        if page.rows.is_empty() && page.has_more {
            anyhow::bail!("Entry query reported more rows without returning a row");
        }
        ids.extend(page.rows.iter().map(|row| row.id.clone()));
        if !page.has_more {
            assert!(
                page.next.is_none(),
                "final EntryQuery page must not carry a continuation"
            );
            break;
        }
        after = Some(
            page.next
                .context("Entry query reported more rows without a continuation")?,
        );
    }
    Ok(ids)
}

fn assert_no_duplicate_or_skip(ids: &[String], expected_count: usize, what: &str) {
    assert_eq!(ids.len(), expected_count, "{what}: traversal row count");
    let unique: BTreeSet<_> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        expected_count,
        "{what}: traversal must not duplicate or skip rows"
    );
}

/// Repeated sort keys (identical priorities) must page without duplicate or
/// skip: the hidden stable entry identity keeps the order total.
#[tokio::test]
async fn repeated_sort_key_pages_by_hidden_stable_identity() -> Result<()> {
    let (service, space_id) =
        setup_space("memory://entry-traversal-repeated-key", "repeated").await?;
    upsert_task_form(&service, &space_id).await?;
    let form = form_id(&service, &space_id, "Task").await?;
    let priority = property_ref(&service, &space_id, "Task", "Priority").await?;

    seed_tasks(
        &service,
        &space_id,
        (0..12).map(|index| (format!("repeat-{index:02}"), 1)),
    )
    .await?;
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: vec![EntrySort {
            field: priority,
            direction: EntrySortDirection::Asc,
        }],
    };
    let one_shot = collect_ids(&service, &space_id, &query, 1_000).await?;
    let paged = collect_ids(&service, &space_id, &query, 3).await?;
    assert_no_duplicate_or_skip(&paged, 12, "repeated sort key");
    assert_eq!(
        paged, one_shot,
        "paged traversal must match the single-shot order"
    );
    let mut expected: Vec<String> = (0..12).map(|index| format!("repeat-{index:02}")).collect();
    expected.sort();
    assert_eq!(
        paged, expected,
        "tied sort keys must fall back to hidden entry-id order"
    );
    // Determinism: a second traversal must produce the identical order.
    assert_eq!(collect_ids(&service, &space_id, &query, 5).await?, paged);
    Ok(())
}

/// Null sort values order `NULLS LAST` and the cursor continuation must resume
/// inside and after the null group without duplicate or skip.
#[tokio::test]
async fn null_sort_values_order_across_cursor_continuation() -> Result<()> {
    let (service, space_id) = setup_space("memory://entry-traversal-null-sort", "nullsort").await?;
    upsert_task_form(&service, &space_id).await?;
    let form = form_id(&service, &space_id, "Task").await?;
    let nickname = property_ref(&service, &space_id, "Task", "Nickname").await?;

    seed_task_fixtures(
        &service,
        &space_id,
        (0..5)
            .map(|index| {
                (
                    format!("nick-{index:02}"),
                    "open".to_owned(),
                    index,
                    Some(format!("name-{index:02}")),
                )
            })
            .chain((0..5).map(|index| {
                (
                    format!("nonull-{index:02}"),
                    "open".to_owned(),
                    100 + index,
                    None,
                )
            })),
    )
    .await?;
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: vec![EntrySort {
            field: nickname,
            direction: EntrySortDirection::Asc,
        }],
    };
    let one_shot = collect_ids(&service, &space_id, &query, 1_000).await?;
    let paged = collect_ids(&service, &space_id, &query, 2).await?;
    assert_no_duplicate_or_skip(&paged, 10, "null sort traversal");
    assert_eq!(
        paged, one_shot,
        "paged traversal must match the single-shot order"
    );
    assert!(
        paged[..5].iter().all(|id| id.starts_with("nick-")),
        "non-null nicknames sort before nulls: {paged:?}"
    );
    assert!(
        paged[5..].iter().all(|id| id.starts_with("nonull-")),
        "null nicknames sort last: {paged:?}"
    );
    Ok(())
}

/// Multi-column sorts (`Status ASC`, `updated_at DESC`, hidden entry id) must
/// hold across page boundaries.
#[tokio::test]
async fn multi_column_sort_survives_page_boundaries() -> Result<()> {
    let (service, space_id) =
        setup_space("memory://entry-traversal-multi-sort", "multisort").await?;
    upsert_task_form(&service, &space_id).await?;
    let form = form_id(&service, &space_id, "Task").await?;
    let status = property_ref(&service, &space_id, "Task", "Status").await?;

    // Interleave source rows, then seed the final values in one publication:
    // Status groups differ, UpdatedAt descends inside each group, and the top
    // two open rows tie so hidden stable identity resolves their order.
    seed_task_fixtures_with_timestamps(
        &service,
        &space_id,
        [
            ("multi-00", "open", 99, None, 1_000_005),
            ("multi-01", "open", 99, None, 1_000_005),
            ("multi-02", "open", 2, None, 1_000_002),
            ("multi-03", "closed", 3, None, 1_000_003),
            ("multi-04", "open", 4, None, 1_000_004),
            ("multi-05", "closed", 5, None, 1_000_005),
        ]
        .into_iter()
        .map(|(id, status, priority, nickname, updated_at)| {
            (
                id.to_owned(),
                status.to_owned(),
                priority,
                nickname.map(str::to_owned),
                updated_at,
            )
        }),
    )
    .await?;
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: vec![
            EntrySort {
                field: status,
                direction: EntrySortDirection::Asc,
            },
            EntrySort {
                field: EntryFieldRef::UpdatedAt,
                direction: EntrySortDirection::Desc,
            },
        ],
    };
    let one_shot = collect_ids(&service, &space_id, &query, 1_000).await?;
    let paged = collect_ids(&service, &space_id, &query, 4).await?;
    assert_no_duplicate_or_skip(&paged, 6, "multi-column sort traversal");
    assert_eq!(
        paged, one_shot,
        "paged traversal must match the single-shot order"
    );
    assert_eq!(
        paged,
        ["multi-05", "multi-03", "multi-00", "multi-01", "multi-04", "multi-02"],
        "page-boundary order must apply Status, UpdatedAt, then stable identity"
    );

    Ok(())
}

/// A cursor pins its immutable publication: live create/update/delete between
/// pages must not move the second page.
#[tokio::test]
async fn mutation_between_pages_still_reads_pinned_publication() -> Result<()> {
    let (service, space_id) = setup_space("memory://entry-traversal-pinned", "pinned").await?;
    upsert_task_form(&service, &space_id).await?;
    let form = form_id(&service, &space_id, "Task").await?;
    let priority = property_ref(&service, &space_id, "Task", "Priority").await?;

    for index in 0..6 {
        create_task(
            &service,
            &space_id,
            &format!("pin-{index:02}"),
            task_fields(Some("open"), Some(index), None),
        )
        .await?;
    }
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: vec![EntrySort {
            field: priority,
            direction: EntrySortDirection::Asc,
        }],
    };
    let first = service
        .query_entry_page(&space_id, page_request(query.clone(), 2, None))
        .await?;
    assert!(first.has_more);
    let cursor = first
        .next
        .clone()
        .context("first page must carry a continuation")?;
    let first_ids: Vec<String> = first.rows.iter().map(|row| row.id.clone()).collect();
    assert_eq!(first_ids, vec!["pin-00", "pin-01"]);

    // Live mutations advance Head after the cursor was issued.
    create_task(
        &service,
        &space_id,
        "pin-new",
        task_fields(Some("open"), Some(-1), None),
    )
    .await?;
    let current = service.get_entry(&space_id, "pin-04").await?;
    let revision = current["revision_id"].as_str().context("entry revision")?;
    service
        .update_structured_entry(
            &space_id,
            "pin-04",
            Some("Task".to_string()),
            task_fields(Some("open"), Some(10_000), None),
            BTreeMap::new(),
            Some(revision),
            "owner",
        )
        .await?;
    service.delete_entry(&space_id, "pin-02", "owner").await?;

    let second = service
        .query_entry_page(&space_id, page_request(query.clone(), 2, Some(cursor)))
        .await?;
    let second_ids: Vec<String> = second.rows.iter().map(|row| row.id.clone()).collect();
    assert_eq!(
        second_ids,
        vec!["pin-02", "pin-03"],
        "continuation must read the pinned publication, not live Head"
    );

    // Draining the rest of the pinned traversal still yields the original six.
    let mut pinned = first_ids.clone();
    pinned.extend(second_ids);
    let mut after = second.next.clone();
    while let Some(token) = after {
        let page = service
            .query_entry_page(&space_id, page_request(query.clone(), 2, Some(token)))
            .await?;
        pinned.extend(page.rows.iter().map(|row| row.id.clone()));
        after = page.next.clone();
        if !page.has_more {
            break;
        }
    }
    assert_eq!(pinned.len(), 6, "pinned traversal must still see six rows");
    assert!(
        !pinned.contains(&"pin-new".to_string()),
        "pinned traversal must not see the live-created entry: {pinned:?}"
    );
    Ok(())
}

/// After cursor issuance the continuation re-evaluates current authorization:
/// a revoked reader cannot use an old cursor to bypass the new policy.
#[tokio::test]
async fn authorization_change_rejects_issued_continuation() -> Result<()> {
    let service = UgoiteService::new("memory://entry-traversal-auth-change")?;
    let owner = Uuid::from_u128(901);
    let viewer = Uuid::from_u128(902);
    let space_id = service
        .create_space_for_principal("auth-change", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "Task",
                "fields": {"Status": {"type": "string"}},
            }),
        )
        .await?;
    for entry_id in ["auth-00", "auth-01", "auth-02", "auth-03"] {
        service
            .create_structured_entry_authorized_for_principals(
                &space_id,
                entry_id,
                "Task".to_string(),
                Vec::new(),
                task_fields(Some("open"), None, None),
                BTreeMap::new(),
                "owner",
                &[owner],
            )
            .await?;
    }
    let authorizer = Authorizer::new(service.operator().clone());
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_owned(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;
    let form = form_id(&service, &space_id, "Task").await?;
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: Vec::new(),
    };
    let request = EntryPageRequest {
        query: query.clone(),
        projection: EntryProjection::Preview,
        limit: 2,
        after: None,
    };
    let first = service
        .query_entry_page_authorized_for_principals(&space_id, &[viewer], request)
        .await?;
    assert!(first.has_more);
    let cursor = first
        .next
        .clone()
        .context("first authorized page must carry a continuation")?;

    // The owner revokes one entry after the cursor was issued. The state
    // revision moves, so the old cursor fingerprint no longer authorizes.
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "auth-02".to_owned(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let continued = service
        .query_entry_page_authorized_for_principals(
            &space_id,
            &[viewer],
            EntryPageRequest {
                query,
                projection: EntryProjection::Preview,
                limit: 2,
                after: Some(cursor),
            },
        )
        .await;
    let error = continued.expect_err("revoked continuation must fail closed");
    let message = format!("{error:#}");
    assert!(
        message.contains("authorization"),
        "revoked continuation must report the authorization change: {message}"
    );
    Ok(())
}

async fn seed_tasks(
    service: &UgoiteService,
    space_id: &str,
    entries: impl Iterator<Item = (String, i64)>,
) -> Result<()> {
    seed_task_fixtures_with_timestamps(
        service,
        space_id,
        entries.enumerate().map(|(index, (external_id, priority))| {
            (
                external_id,
                "open".to_owned(),
                priority,
                None,
                1_000_000 + index as i64,
            )
        }),
    )
    .await
}

/// Seed the richer sorting fixtures through the same single-publication
/// revision batch path as `seed_tasks`, including explicitly tied sort tuples.
async fn seed_task_fixtures(
    service: &UgoiteService,
    space_id: &str,
    entries: impl Iterator<Item = (String, String, i64, Option<String>)>,
) -> Result<()> {
    seed_task_fixtures_with_timestamps(
        service,
        space_id,
        entries
            .enumerate()
            .map(|(index, (id, status, priority, nickname))| {
                (id, status, priority, nickname, 1_000_000 + index as i64)
            }),
    )
    .await
}

async fn seed_task_fixtures_with_timestamps(
    service: &UgoiteService,
    space_id: &str,
    entries: impl Iterator<Item = (String, String, i64, Option<String>, i64)>,
) -> Result<()> {
    use ugoite_domain::change::ChangeCommand;
    use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
    use ugoite_domain::id::{EntryId, FieldId};
    use ugoite_iceberg::{iceberg_store, publication_context_for_change};

    let workspace =
        iceberg_store::native_mutation_workspace(service.operator(), &format!("spaces/{space_id}"))
            .await?;
    let form = workspace
        .list_forms()
        .await?
        .into_iter()
        .find(|form| form.name == "Task")
        .context("Task form")?;
    let field_ids = form
        .fields
        .iter()
        .map(|field| {
            (
                field.name.as_str(),
                FieldId::new(field.id.get()).expect("field id"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let revisions = entries
        .enumerate()
        .map(
            |(index, (external_id, status, priority, nickname, updated_at))| {
                let values = BTreeMap::from([
                    (field_ids["Status"], FieldValue::String(status)),
                    (field_ids["Priority"], FieldValue::Integer(priority)),
                ]);
                let values = if let Some(nickname) = nickname {
                    let mut values = values;
                    values.insert(field_ids["Nickname"], FieldValue::String(nickname));
                    values
                } else {
                    values
                };
                EntryRevision {
                    form_id: form.id,
                    entry_id: EntryId::from(Uuid::from_u128(700_000 + index as u128)),
                    revision_id: Uuid::from_u128(800_000 + index as u128).into(),
                    parent_revision_id: None,
                    entry_version: 1,
                    change_id: "traversal-fixture-seed".to_owned(),
                    expected_version: None,
                    operation: EntryOperation::Upsert,
                    committed_at_micros: updated_at,
                    author_id: "owner".to_owned(),
                    form_version: form.version,
                    source_kind: "test".to_owned(),
                    source_id: None,
                    entry: EntryMetadata {
                        external_id,
                        created_at_micros: updated_at,
                        updated_at_micros: updated_at,
                        updated_by: "owner".to_owned(),
                        ..EntryMetadata::default()
                    },
                    values,
                    extra_attributes: BTreeMap::new(),
                    extension_metadata: BTreeMap::new(),
                }
            },
        )
        .collect::<Vec<_>>();
    let command = ChangeCommand {
        change_id: "traversal-fixture-seed".to_owned(),
        run_id: None,
        actor_principal_id: "owner".to_owned(),
        message: Some("seed traversal sorting fixture".to_owned()),
        reverts_change_id: None,
        created_at_micros: 1,
    };
    workspace
        .commit(publication_context_for_change(
            &command,
            "test.entry.traversal",
            &revisions,
        )?)?
        .append_revisions(form.id, revisions)
        .await?;

    Ok(())
}

/// Small incomplete final page confirms the last cursor terminates correctly.
#[tokio::test]
async fn traversal_covers_a_partial_final_page_without_gap() -> Result<()> {
    const COUNT: usize = 21;
    const PAGE: usize = 4;

    let (service, space_id) = setup_space("memory://entry-traversal-partial", "partial").await?;
    upsert_task_form(&service, &space_id).await?;
    let form = form_id(&service, &space_id, "Task").await?;
    seed_tasks(
        &service,
        &space_id,
        (0..COUNT).map(|index| (format!("bulk-{index:05}"), index as i64)),
    )
    .await?;

    let priority = property_ref(&service, &space_id, "Task", "Priority").await?;
    let query = EntryQuery {
        scope: EntryQueryScope::Form { form_id: form },
        text: None,
        filters: Vec::new(),
        sort: vec![EntrySort {
            field: priority,
            direction: EntrySortDirection::Asc,
        }],
    };
    let ids = collect_ids(&service, &space_id, &query, PAGE).await?;
    assert_no_duplicate_or_skip(&ids, COUNT, "partial final page traversal");
    let expected: Vec<String> = (0..COUNT).map(|index| format!("bulk-{index:05}")).collect();
    assert_eq!(
        ids, expected,
        "numeric priority order must survive the partial last page"
    );
    Ok(())
}
