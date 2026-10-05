//! PR5 (Follow-up H, issue #2989) stateless SQL follow-up coverage.
//!
//! `SqlQuery` stays a stateless read-only operation: the first page pins the
//! immutable publication, the opaque continuation carries only the query
//! coordinate, counting is an explicit separate operation, and parameters are
//! always typed values (never string substitution). These tests exercise the
//! real backend (memory operator through `UgoiteService::query_sql`):
//!
//! - first page + continuation cover the full ordered result;
//! - explicit `count_sql` matches the full result size;
//! - declared parameter types filter, declared `long`/`double` values and
//!   inferred `long`/`double` numbers bind, and typed nulls bind for
//!   `string`/`long`/`double` without substitution;
//! - reusing a continuation after the SQL text, parameters, or parameter
//!   types change resets (fails closed) instead of reading the old
//!   coordinate;
//! - write statements stay read-only and the continuation stays opaque.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryCountRequest, SqlQueryRequest};
use ugoite_iceberg::entry::{self, IntegrityPayload};
use ugoite_iceberg::integrity::{IntegrityProvider, RealIntegrityProvider};
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::UgoiteService;

struct SqlSpace {
    service: UgoiteService,
    space_id: String,
    relation: String,
    form_name: String,
    status_column: String,
    priority_column: String,
}

async fn setup_sql_space(uri: &str, slug: &str) -> Result<SqlSpace> {
    setup_sql_space_with_form_name(uri, slug, "Task").await
}

async fn setup_sql_space_with_form_name(
    uri: &str,
    slug: &str,
    form_name: &str,
) -> Result<SqlSpace> {
    let service = UgoiteService::new(uri)?;
    service.create_space(slug).await?;
    let space_id = slug.to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "Status": {"type": "string"},
                    "Priority": {"type": "long"},
                },
            }),
        )
        .await?;
    for (entry_id, status, priority) in [
        ("sql-00", "open", 1),
        ("sql-01", "closed", 2),
        ("sql-02", "open", 3),
        ("sql-03", "closed", 4),
        ("sql-04", "open", 5),
    ] {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                entry_id,
                form_name.to_string(),
                Vec::new(),
                BTreeMap::from([
                    ("Status".to_string(), Value::String(status.to_string())),
                    ("Priority".to_string(), json!(priority)),
                ]),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }
    let form_json = service.get_form(&space_id, form_name).await?;
    let form_id: ugoite_domain::id::FormId =
        serde_json::from_value(form_json["id"].clone()).context("Form id")?;
    let field_id = |name: &str| -> Result<i32> {
        form_json
            .pointer(&format!("/fields/{name}/id"))
            .and_then(Value::as_i64)
            .and_then(|id| i32::try_from(id).ok())
            .with_context(|| format!("Form field {name} is missing its id"))
    };
    Ok(SqlSpace {
        service,
        space_id,
        relation: ugoite_domain::form::sql_relation_name(form_id),
        form_name: form_name.to_string(),
        status_column: format!("field_{}", field_id("Status")?),
        priority_column: format!("field_{}", field_id("Priority")?),
    })
}

#[tokio::test]
async fn quoted_form_name_resolves_for_page_and_count() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-form-name", "sqlformname").await?;
    let named_sql = format!(
        "WITH task_rows AS (SELECT \"_ugoite_id\" FROM \"{}\") \
         SELECT task_rows.\"_ugoite_id\" FROM task_rows \
         JOIN (SELECT \"_ugoite_id\" FROM \"{}\") AS nested_rows \
         ON task_rows.\"_ugoite_id\" = nested_rows.\"_ugoite_id\" \
         ORDER BY task_rows.\"_ugoite_id\"",
        space.form_name, space.form_name
    );
    let page = space
        .service
        .query_sql(&space.space_id, query_request(named_sql.clone(), 10))
        .await
        .unwrap_or_else(|error| panic!("named Form page failed: {error:#}"));
    assert_eq!(page.rows.len(), 5);
    let count = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: named_sql,
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                saved_sql: None,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("named Form count failed: {error:#}"));
    assert_eq!(count, 5);

    let unquoted = format!("SELECT * FROM {}", space.form_name);
    assert!(space
        .service
        .query_sql(&space.space_id, query_request(unquoted, 10))
        .await
        .is_err());
    let wrong_case = format!("SELECT * FROM \"{}\"", space.form_name.to_lowercase());
    assert!(space
        .service
        .query_sql(&space.space_id, query_request(wrong_case, 10))
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn quoted_form_name_with_digits_hyphen_and_parameter_resolves() -> Result<()> {
    let space = setup_sql_space_with_form_name(
        "memory://sql-stateless-form-name-special",
        "sqlformnamespecial",
        "2026-Expense",
    )
    .await?;
    let sql = format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $priority \
         AND '2026-Expense' = '2026-Expense' -- FROM \"missing\"\n         ORDER BY \"_ugoite_id\"",
        space.form_name, space.priority_column
    );
    let page = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql,
                parameters: Map::from_iter([("priority".into(), json!(3))]),
                parameter_types: BTreeMap::new(),
                limit: 10,
                continuation: None,
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0]["_ugoite_id"], "sql-02");
    Ok(())
}

#[tokio::test]
async fn quoted_form_names_differing_only_by_case_resolve_to_their_own_rows() -> Result<()> {
    let space = setup_sql_space_with_form_name(
        "memory://sql-stateless-form-name-case",
        "sqlformnamecase",
        "Ledger",
    )
    .await?;
    // A second Form whose name differs only by ASCII case keeps its own row;
    // each quoted reference must resolve to its own Form.
    space
        .service
        .upsert_form(
            &space.space_id,
            &json!({
                "name": "ledger",
                "fields": {
                    "Status": {"type": "string"},
                    "Priority": {"type": "long"},
                },
            }),
        )
        .await?;
    space
        .service
        .create_structured_entry_with_receipt(
            &space.space_id,
            "sql-lower-00",
            "ledger".to_string(),
            Vec::new(),
            BTreeMap::from([
                ("Status".to_string(), Value::String("open".to_string())),
                ("Priority".to_string(), json!(1)),
            ]),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let upper = space
        .service
        .query_sql(
            &space.space_id,
            query_request(
                "SELECT \"_ugoite_id\" FROM \"Ledger\" ORDER BY \"_ugoite_id\"".to_string(),
                10,
            ),
        )
        .await?;
    assert_eq!(upper.rows.len(), 5);
    assert!(upper.rows.iter().all(|row| row["_ugoite_id"]
        .as_str()
        .is_some_and(|id| id.starts_with("sql-0"))));
    let lower = space
        .service
        .query_sql(
            &space.space_id,
            query_request(
                "SELECT \"_ugoite_id\" FROM \"ledger\" ORDER BY \"_ugoite_id\"".to_string(),
                10,
            ),
        )
        .await?;
    assert_eq!(lower.rows.len(), 1);
    assert_eq!(lower.rows[0]["_ugoite_id"], "sql-lower-00");
    // Any other ASCII case still rejects instead of resolving either Form.
    assert!(space
        .service
        .query_sql(
            &space.space_id,
            query_request("SELECT * FROM \"LEDGER\"".to_string(), 10),
        )
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn saved_sql_revision_uses_fixed_form_binding_and_continuation_identity() -> Result<()> {
    let space = setup_sql_space("memory://sql-saved-binding", "sqlsavedbinding").await?;
    let sql = format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" ORDER BY \"_ugoite_id\"",
        space.form_name
    );
    let saved = space
        .service
        .create_saved_sql(
            &space.space_id,
            Some("saved-by-form-name"),
            &SqlPayload {
                name: Some("Tasks".into()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: sql.clone(),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let source = SavedSqlRevisionRef {
        id: "saved-by-form-name".into(),
        revision_id: saved["revision_id"].as_str().context("revision id")?.into(),
    };
    assert_eq!(saved["metadata"]["bindingVersion"], 1);
    assert_eq!(saved["metadata"]["formBindings"][0]["name"], "Task");

    let first = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                limit: 2,
                continuation: None,
                saved_sql: Some(source.clone()),
            },
        )
        .await?;
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    let next = first.next.clone().context("saved SQL continuation")?;
    let updated = space
        .service
        .update_saved_sql(
            &space.space_id,
            "saved-by-form-name",
            &SqlPayload {
                name: Some("Tasks".into()),
                kind: SqlKind::UserQuery,
                metadata: Some(serde_json::from_value(saved["metadata"].clone())?),
                sql: format!(
                "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = 'open' ORDER BY \"_ugoite_id\"",
                space.form_name, space.status_column
            ),
                variables: json!([]),
            },
            &source.revision_id,
            "owner",
        )
        .await?;
    let updated_source = SavedSqlRevisionRef {
        id: source.id.clone(),
        revision_id: updated["revision_id"]
            .as_str()
            .context("updated revision id")?
            .into(),
    };
    assert_eq!(updated["metadata"]["bindingVersion"], 1);
    assert_eq!(
        updated["metadata"]["formBindings"], saved["metadata"]["formBindings"],
        "the update persists server-derived Form bindings from the edited SQL"
    );
    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: Some(next),
                ..query_request(String::new(), 2)
            },
        )
        .await;
    assert!(
        second.is_err(),
        "continuation without its saved revision must fail closed"
    );
    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: first.next.clone(),
                saved_sql: Some(source.clone()),
                ..query_request(String::new(), 2)
            },
        )
        .await?;
    assert_eq!(second.rows.len(), 2);

    let count = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                saved_sql: Some(source.clone()),
            },
        )
        .await?;
    assert_eq!(count, 5);
    let updated_count = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                saved_sql: Some(updated_source),
            },
        )
        .await?;
    assert_eq!(updated_count, 3);

    let mismatched = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: "SELECT 1".into(),
                saved_sql: Some(source),
                ..query_request(String::new(), 2)
            },
        )
        .await;
    assert!(
        mismatched.is_err(),
        "caller SQL cannot replace the selected revision"
    );
    Ok(())
}

fn base_sql(space: &SqlSpace) -> String {
    format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" ORDER BY \"_ugoite_id\"",
        space.relation
    )
}

fn query_request(sql: String, limit: usize) -> SqlQueryRequest {
    SqlQueryRequest {
        sql,
        parameters: Map::new(),
        parameter_types: BTreeMap::new(),
        limit,
        continuation: None,
        saved_sql: None,
    }
}

#[tokio::test]
async fn listing_empty_saved_sql_does_not_create_its_form() -> Result<()> {
    let service = UgoiteService::new("memory://sql-read-only-list")?;
    service.create_space("sqlreadonlylist").await?;

    let saved_sql = service
        .list_saved_sql_operator_unscoped("sqlreadonlylist")
        .await?;
    assert!(saved_sql.is_empty());
    assert!(service.get_form("sqlreadonlylist", "SQL").await.is_err());
    Ok(())
}

#[tokio::test]
async fn synthetic_prebinding_revision_reads_and_runs_without_rewrite() -> Result<()> {
    let space = setup_sql_space(
        "memory://sql-synthetic-prebinding",
        "sqlsyntheticprebinding",
    )
    .await?;
    // This row models the pre-binding generic Entry representation directly.
    // It intentionally bypasses create_saved_sql, which writes current bindings.
    let bootstrap = space
        .service
        .create_saved_sql(
            &space.space_id,
            Some("bootstrap_sql_form"),
            &SqlPayload {
                name: Some("Bootstrap".into()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: "SELECT 1".into(),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let legacy_sql = format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" >= $minimum \
         ORDER BY \"_ugoite_id\"",
        space.relation, space.priority_column
    );
    let operator = space.service.operator();
    let workspace_path = space.service.workspace_path(&space.space_id);
    let integrity = RealIntegrityProvider::from_space(operator, &space.space_id).await?;
    let bootstrap_revision_id = bootstrap["revision_id"].as_str().context("revision id")?;
    let mut legacy_revision: entry::RevisionRow = serde_json::from_value(
        entry::get_entry_revision(
            operator,
            &workspace_path,
            "bootstrap_sql_form",
            bootstrap_revision_id,
        )
        .await?,
    )?;
    let legacy_variables = json!([{
        "name": "minimum",
        "type": "integer",
        "description": ""
    }]);
    let canonical_payload = json!({
        "name": "Historical query",
        "kind": "user-query",
        "metadata": null,
        "sql": legacy_sql.clone(),
        "variables": legacy_variables.clone(),
    });
    let canonical_payload = serde_json::to_string(&canonical_payload)?;
    let revision_integrity = IntegrityPayload {
        checksum: integrity.checksum(&canonical_payload),
        signature: integrity.signature(&canonical_payload),
    };
    let revision_id = "01900000-0000-7000-8000-000000000123".to_string();
    legacy_revision.revision_id = revision_id.clone();
    legacy_revision.change_id = "01900000-0000-7000-8000-000000000124".into();
    legacy_revision.entry_id = "synthetic_prebinding_revision".into();
    legacy_revision.parent_revision_id = None;
    legacy_revision.timestamp = Utc::now().timestamp_millis() as f64;
    legacy_revision.author = "owner".into();
    legacy_revision.updated_by = "owner".into();
    legacy_revision.deleted_by = None;
    legacy_revision.fields = json!({
        "name": "Historical query",
        "sql": legacy_sql.clone(),
        "variables": legacy_variables.clone(),
    });
    legacy_revision.extra_attributes = json!({"kind":"user-query", "metadata":null});
    legacy_revision.markdown_checksum = revision_integrity.checksum.clone();
    legacy_revision.integrity = revision_integrity.clone();
    legacy_revision.entry_version = 1;
    legacy_revision.operation = "upsert".into();
    if let Some(state) = legacy_revision.state.as_mut() {
        state.entry_id = "synthetic_prebinding_revision".into();
        state.revision_id = revision_id.clone();
        state.parent_revision_id = None;
        state.fields = legacy_revision.fields.clone();
        state.extra_attributes = legacy_revision.extra_attributes.clone();
        state.integrity = revision_integrity;
        state.created_at = legacy_revision.timestamp;
        state.updated_at = legacy_revision.timestamp;
        state.author = "owner".into();
        state.updated_by = "owner".into();
        state.deleted_by = None;
        state.deleted = false;
        state.deleted_at = None;
        state.entry_version = 1;
    }
    entry::append_revision_batch_for_form(operator, &workspace_path, "SQL", &[legacy_revision])
        .await?;
    let saved = space
        .service
        .get_saved_sql(&space.space_id, "synthetic_prebinding_revision")
        .await?;
    let saved_revision_id = saved["revision_id"].as_str().context("revision id")?;
    assert_eq!(saved_revision_id, revision_id);
    let before_entry = entry::get_entry_revision(
        operator,
        &workspace_path,
        "synthetic_prebinding_revision",
        saved_revision_id,
    )
    .await?;
    assert_eq!(saved["sql"], legacy_sql);
    assert_eq!(
        saved["variables"],
        json!([{"name":"minimum","type":"integer","description":""}])
    );
    assert!(saved["metadata"].is_null());

    let appended = space
        .service
        .update_saved_sql(
            &space.space_id,
            "synthetic_prebinding_revision",
            &SqlPayload {
                name: Some("Historical query".into()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: legacy_sql.clone(),
                variables: legacy_variables.clone(),
            },
            saved_revision_id,
            "owner",
        )
        .await?;
    assert_ne!(appended["revision_id"], saved_revision_id);

    let before_form = space.service.get_form(&space.space_id, "SQL").await?;
    let before_space = space.service.get_space(&space.space_id).await?;
    let before_pins = space.service.list_pins(&space.space_id).await?;
    let first = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(3))]),
                parameter_types: BTreeMap::new(),
                limit: 2,
                continuation: None,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: "synthetic_prebinding_revision".into(),
                    revision_id: saved_revision_id.into(),
                }),
            },
        )
        .await?;
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(3))]),
                parameter_types: BTreeMap::new(),
                limit: 2,
                continuation: first.next,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: "synthetic_prebinding_revision".into(),
                    revision_id: saved_revision_id.into(),
                }),
            },
        )
        .await?;
    assert_eq!(
        first
            .rows
            .iter()
            .chain(&second.rows)
            .map(|row| row["_ugoite_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["sql-02", "sql-03", "sql-04"]
    );
    let count = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(3))]),
                parameter_types: BTreeMap::new(),
                saved_sql: Some(SavedSqlRevisionRef {
                    id: "synthetic_prebinding_revision".into(),
                    revision_id: saved_revision_id.into(),
                }),
            },
        )
        .await?;
    assert_eq!(count, 3);
    assert!(space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                saved_sql: Some(SavedSqlRevisionRef {
                    id: "synthetic_prebinding_revision".into(),
                    revision_id: "wrong-revision".into(),
                }),
                ..query_request(String::new(), 2)
            }
        )
        .await
        .is_err());

    let after = space
        .service
        .get_saved_sql(&space.space_id, "synthetic_prebinding_revision")
        .await?;
    assert_eq!(after["revision_id"], appended["revision_id"]);
    assert_eq!(after["sql"], saved["sql"]);
    assert_eq!(after["variables"], saved["variables"]);
    assert_eq!(after["metadata"]["bindingVersion"], 1);
    assert_eq!(before_entry["extra_attributes"]["metadata"], Value::Null);
    let after_entry = entry::get_entry_revision(
        operator,
        &workspace_path,
        "synthetic_prebinding_revision",
        saved_revision_id,
    )
    .await?;
    assert_eq!(after_entry["revision_id"], before_entry["revision_id"]);
    assert_eq!(after_entry["integrity"], before_entry["integrity"]);
    assert_eq!(after_entry["fields"], before_entry["fields"]);
    assert_eq!(
        space.service.get_form(&space.space_id, "SQL").await?,
        before_form
    );
    assert_eq!(
        space.service.get_space(&space.space_id).await?,
        before_space
    );
    assert_eq!(space.service.list_pins(&space.space_id).await?, before_pins);
    Ok(())
}

#[tokio::test]
async fn synthetic_prebinding_revision_rejections_fail_closed() -> Result<()> {
    let space = setup_sql_space(
        "memory://sql-synthetic-prebinding-rejections",
        "sqlsyntheticprebindingrejections",
    )
    .await?;
    // Create the SQL Form using the current service, then seed each tested
    // legacy SQL revision directly as a generic Entry revision.
    let bootstrap = space
        .service
        .create_saved_sql(
            &space.space_id,
            Some("bootstrap_sql_form"),
            &SqlPayload {
                name: Some("Bootstrap".into()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: "SELECT 1".into(),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let operator = space.service.operator();
    let workspace_path = space.service.workspace_path(&space.space_id);
    let integrity = RealIntegrityProvider::from_space(operator, &space.space_id).await?;
    let template: entry::RevisionRow = serde_json::from_value(
        entry::get_entry_revision(
            operator,
            &workspace_path,
            "bootstrap_sql_form",
            bootstrap["revision_id"].as_str().context("revision id")?,
        )
        .await?,
    )?;

    let seed_revision = |entry_id: &str,
                         revision_id: &str,
                         change_id: &str,
                         sql: &str,
                         signed_sql: &str,
                         metadata: Value|
     -> Result<entry::RevisionRow> {
        let variables = json!([]);
        let fields = json!({
            "name": "Synthetic pre-binding query",
            "sql": sql,
            "variables": variables,
        });
        let payload = json!({
            "name": "Synthetic pre-binding query",
            "kind": "user-query",
            "metadata": metadata,
            "sql": signed_sql,
            "variables": variables,
        });
        let serialized = serde_json::to_string(&payload)?;
        let revision_integrity = IntegrityPayload {
            checksum: integrity.checksum(&serialized),
            signature: integrity.signature(&serialized),
        };
        let extra_attributes = json!({"kind": "user-query", "metadata": metadata});
        let timestamp = Utc::now().timestamp_millis() as f64;
        let mut row = template.clone();
        row.revision_id = revision_id.to_string();
        row.change_id = change_id.to_string();
        row.entry_id = entry_id.to_string();
        row.parent_revision_id = None;
        row.timestamp = timestamp;
        row.author = "owner".into();
        row.updated_by = "owner".into();
        row.deleted_by = None;
        row.fields = fields.clone();
        row.extra_attributes = extra_attributes.clone();
        row.markdown_checksum = revision_integrity.checksum.clone();
        row.integrity = revision_integrity.clone();
        row.entry_version = 1;
        row.operation = "upsert".into();
        if let Some(state) = row.state.as_mut() {
            state.entry_id = entry_id.into();
            state.revision_id = revision_id.into();
            state.parent_revision_id = None;
            state.fields = fields;
            state.extra_attributes = extra_attributes;
            state.integrity = revision_integrity;
            state.created_at = timestamp;
            state.updated_at = timestamp;
            state.author = "owner".into();
            state.updated_by = "owner".into();
            state.deleted_by = None;
            state.deleted = false;
            state.deleted_at = None;
            state.entry_version = 1;
        }
        Ok(row)
    };

    let tampered_id = "synthetic_prebinding_tampered";
    let tampered_revision_id = "01900000-0000-7000-8000-000000000201";
    let incomplete_id = "synthetic_prebinding_incomplete";
    let incomplete_revision_id = "01900000-0000-7000-8000-000000000202";
    let quoted_id = "synthetic_prebinding_quoted_name";
    let quoted_revision_id = "01900000-0000-7000-8000-000000000203";
    let tampered_sql = "SELECT 2";
    let signed_sql = "SELECT 1";
    let incomplete_sql = "SELECT 1";
    let quoted_sql = "SELECT \"_ugoite_id\" FROM \"Task\" ORDER BY \"_ugoite_id\"";
    let revisions = vec![
        // The stored body differs from the body covered by its checksum and
        // signature, so the reader must reject it before execution.
        seed_revision(
            tampered_id,
            tampered_revision_id,
            "01900000-0000-7000-8000-000000000211",
            tampered_sql,
            signed_sql,
            Value::Null,
        )?,
        // A valid integrity envelope does not make a one-sided binding
        // declaration complete enough to infer an old revision's Form ID.
        seed_revision(
            incomplete_id,
            incomplete_revision_id,
            "01900000-0000-7000-8000-000000000211",
            incomplete_sql,
            incomplete_sql,
            json!({"bindingVersion": 1}),
        )?,
        // The quoted Form name has no historical Form ID evidence and must
        // not be resolved against today's Form names.
        seed_revision(
            quoted_id,
            quoted_revision_id,
            "01900000-0000-7000-8000-000000000211",
            quoted_sql,
            quoted_sql,
            Value::Null,
        )?,
    ];
    entry::append_revision_batch_for_form(operator, &workspace_path, "SQL", &revisions).await?;

    let query_saved = |id: &str, revision_id: &str| SqlQueryRequest {
        sql: String::new(),
        parameters: Map::new(),
        parameter_types: BTreeMap::new(),
        limit: 10,
        continuation: None,
        saved_sql: Some(SavedSqlRevisionRef {
            id: id.into(),
            revision_id: revision_id.into(),
        }),
    };
    let missing_revision_error = space
        .service
        .query_sql(
            &space.space_id,
            query_saved(tampered_id, "01900000-0000-7000-8000-000000000299"),
        )
        .await
        .expect_err("an exact id@revision_id miss must not select another revision");
    assert!(format!("{missing_revision_error:#}")
        .contains("synthetic_prebinding_tampered@01900000-0000-7000-8000-000000000299"));

    let tampered_error = space
        .service
        .query_sql(
            &space.space_id,
            query_saved(tampered_id, tampered_revision_id),
        )
        .await
        .expect_err("tampered legacy SQL integrity must be rejected");
    assert!(format!("{tampered_error:#}").contains("Saved SQL integrity mismatch"));

    let incomplete_error = space
        .service
        .query_sql(
            &space.space_id,
            query_saved(incomplete_id, incomplete_revision_id),
        )
        .await
        .expect_err("partial Form binding metadata must be rejected");
    assert!(format!("{incomplete_error:#}")
        .contains("unsupported or incomplete Saved SQL Form binding metadata"));

    let quoted_error = space
        .service
        .query_sql(&space.space_id, query_saved(quoted_id, quoted_revision_id))
        .await
        .expect_err("pre-binding quoted Form names must not be rebound");
    assert!(format!("{quoted_error:#}").contains("LEGACY_SQL_BINDING_UNAVAILABLE"));
    Ok(())
}
/// First page + continuation union the full ordered result on the real
/// backend, with the `has_more`/`next` protocol holding on every page.
#[tokio::test]
async fn stateless_first_page_and_continuation_cover_full_result() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-pages", "sqlpages").await?;
    let full = space
        .service
        .query_sql(&space.space_id, query_request(base_sql(&space), 1_000))
        .await?;
    assert_eq!(full.rows.len(), 5);
    assert!(!full.has_more);
    assert!(full.next.is_none());

    let first = space
        .service
        .query_sql(&space.space_id, query_request(base_sql(&space), 2))
        .await?;
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    let continuation = first
        .next
        .clone()
        .context("first page must carry a continuation")?;
    assert_eq!(first.rows, full.rows[..2].to_vec());

    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: Some(continuation),
                ..query_request(base_sql(&space), 2)
            },
        )
        .await?;
    assert_eq!(second.rows, full.rows[2..4].to_vec());
    assert!(second.has_more);

    let last = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: second.next.clone(),
                ..query_request(base_sql(&space), 2)
            },
        )
        .await?;
    assert_eq!(last.rows, full.rows[4..].to_vec());
    assert!(!last.has_more);
    assert!(last.next.is_none());
    Ok(())
}

/// The explicit count operation matches the full result size without paging.
#[tokio::test]
async fn stateless_explicit_count_matches_full_result() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-count", "sqlcount").await?;
    let count = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: base_sql(&space),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(count, 5);
    let filtered = space
        .service
        .count_sql(
            &space.space_id,
            SqlQueryCountRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = 'open'",
                    space.relation, space.status_column
                ),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(filtered, 3);
    Ok(())
}

/// Parameters bind as typed values: declared `string` filters, inferred
/// `long` numbers bind (Form-type alias for `int64`), and a declared typed
/// null binds without string substitution.
#[tokio::test]
async fn stateless_parameters_bind_with_types_and_typed_null() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-params", "sqlparams").await?;

    // Declared string parameter.
    let filtered = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $status ORDER BY \"_ugoite_id\"",
                    space.relation, space.status_column
                ),
                parameters: Map::from_iter([(
                    "status".to_string(),
                    Value::String("open".to_string()),
                )]),
                parameter_types: BTreeMap::from([("status".to_string(), "string".to_string())]),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await?;
    let ids: Vec<&str> = filtered
        .rows
        .iter()
        .filter_map(|row| row.get("_ugoite_id").and_then(Value::as_str))
        .collect();
    assert_eq!(ids, vec!["sql-00", "sql-02", "sql-04"]);

    // Inferred `long` number parameter (no declared type): the JSON number
    // inference uses Form type names, which bind as typed `int64`.
    let by_priority = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $priority ORDER BY \"_ugoite_id\"",
                    space.relation, space.priority_column
                ),
                parameters: Map::from_iter([("priority".to_string(), json!(3))]),
                parameter_types: BTreeMap::new(),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await?;
    assert_eq!(by_priority.rows.len(), 1);
    assert_eq!(
        by_priority.rows[0].get("_ugoite_id"),
        Some(&json!("sql-02"))
    );

    // Declared typed null binds as a typed null (no substitution, no error).
    let typed_null = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $probe IS NULL ORDER BY \"_ugoite_id\"",
                    space.relation
                ),
                parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                parameter_types: BTreeMap::from([("probe".to_string(), "string".to_string())]),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await?;
    assert_eq!(typed_null.rows.len(), 5);

    // An untyped null and a non-scalar parameter fail closed.
    let untyped_null = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $probe IS NULL ORDER BY \"_ugoite_id\"",
                    space.relation
                ),
                parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                parameter_types: BTreeMap::new(),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await;
    assert!(
        format!("{:#}", untyped_null.expect_err("untyped null must fail"))
            .contains("declared parameter type"),
        "untyped null must demand a declared type"
    );
    let array_param = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $probe IS NULL ORDER BY \"_ugoite_id\"",
                    space.relation
                ),
                parameters: Map::from_iter([("probe".to_string(), json!([1, 2]))]),
                parameter_types: BTreeMap::from([("probe".to_string(), "string".to_string())]),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await;
    assert!(
        format!("{:#}", array_param.expect_err("array parameter must fail"))
            .contains("does not match"),
        "non-scalar parameters must be rejected as a type mismatch"
    );
    // A value that does not match its declared type fails closed.
    let mismatched = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $priority ORDER BY \"_ugoite_id\"",
                    space.relation, space.priority_column
                ),
                parameters: Map::from_iter([("priority".to_string(), Value::String("3".to_string()))]),
                parameter_types: BTreeMap::from([("priority".to_string(), "int64".to_string())]),
                limit: 1_000,
                continuation: None,
            saved_sql: None,
            },
        )
        .await;
    assert!(
        format!("{:#}", mismatched.expect_err("mismatched scalar must fail"))
            .contains("does not match"),
        "invalid scalars must be rejected"
    );
    Ok(())
}

/// Declared `long`/`double` values, inferred `double` numbers, and typed
/// nulls for `long`/`double` bind as typed values alongside the declared
/// `string` and inferred `long` cases covered above.
#[tokio::test]
async fn stateless_parameters_bind_declared_long_double_and_typed_nulls() -> Result<()> {
    let space =
        setup_sql_space("memory://sql-stateless-numeric-params", "sqlnumericparams").await?;

    // Declared `long` value binds as typed `int64`.
    let by_priority = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $priority ORDER BY \"_ugoite_id\"",
                    space.relation, space.priority_column
                ),
                parameters: Map::from_iter([("priority".to_string(), json!(3))]),
                parameter_types: BTreeMap::from([("priority".to_string(), "long".to_string())]),
                limit: 1_000,
                continuation: None,
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(by_priority.rows.len(), 1);
    assert_eq!(
        by_priority.rows[0].get("_ugoite_id"),
        Some(&json!("sql-02"))
    );

    // Declared and inferred `double` numbers bind as typed `float64`.
    for (name, parameter_types) in [
        (
            "declared",
            BTreeMap::from([("ratio".to_string(), "double".to_string())]),
        ),
        ("inferred", BTreeMap::new()),
    ] {
        let page = space
            .service
            .query_sql(
                &space.space_id,
                SqlQueryRequest {
                    sql: format!(
                        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $ratio > 2.0 ORDER BY \"_ugoite_id\"",
                        space.relation
                    ),
                    parameters: Map::from_iter([("ratio".to_string(), json!(2.5))]),
                    parameter_types,
                    limit: 1_000,
                    continuation: None,
                    saved_sql: None,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{name} double parameter failed: {error:#}"));
        assert_eq!(page.rows.len(), 5, "{name} double must bind");
    }

    // Typed nulls bind for `long` and `double` without substitution.
    for kind in ["long", "double"] {
        let typed_null = space
            .service
            .query_sql(
                &space.space_id,
                SqlQueryRequest {
                    sql: format!(
                        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $probe IS NULL ORDER BY \"_ugoite_id\"",
                        space.relation
                    ),
                    parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                    parameter_types: BTreeMap::from([("probe".to_string(), kind.to_string())]),
                    limit: 1_000,
                    continuation: None,
                    saved_sql: None,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("typed null {kind} failed: {error:#}"));
        assert_eq!(typed_null.rows.len(), 5, "typed null {kind} must bind");
    }
    Ok(())
}

/// Reusing a continuation after only the parameter types change fails
/// closed: the parameter fingerprint covers the effective types, so the
/// continuation resets instead of reading the old coordinate.
#[tokio::test]
async fn stateless_continuation_resets_on_parameter_type_change() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-type-reset", "sqltypereset").await?;
    let null_sql = format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE $probe IS NULL ORDER BY \"_ugoite_id\"",
        space.relation
    );
    let first = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: null_sql.clone(),
                parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                parameter_types: BTreeMap::from([("probe".to_string(), "string".to_string())]),
                limit: 2,
                continuation: None,
                saved_sql: None,
            },
        )
        .await?;
    assert!(first.has_more);
    let continuation = first
        .next
        .clone()
        .context("first page must carry a continuation")?;

    // Same SQL text and same values under the identical types still continue.
    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: null_sql.clone(),
                parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                parameter_types: BTreeMap::from([("probe".to_string(), "string".to_string())]),
                limit: 2,
                continuation: Some(continuation.clone()),
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(
        second
            .rows
            .iter()
            .filter_map(|row| row.get("_ugoite_id").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        vec!["sql-02", "sql-03"]
    );

    // Same SQL text and same values with only the declared type changed.
    let changed_types = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: null_sql,
                parameters: Map::from_iter([("probe".to_string(), Value::Null)]),
                parameter_types: BTreeMap::from([("probe".to_string(), "long".to_string())]),
                limit: 2,
                continuation: Some(continuation),
                saved_sql: None,
            },
        )
        .await;
    assert!(
        format!("{:#}", changed_types.expect_err("changed types must fail"))
            .contains("fingerprint"),
        "changed parameter types with an old continuation must fail on the fingerprint"
    );
    Ok(())
}

/// Reusing a continuation after the SQL text, parameters, or parameter types
/// change fails closed: the continuation resets instead of reading the old
/// coordinate.
#[tokio::test]
async fn stateless_continuation_resets_on_context_change() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-reset", "sqlreset").await?;
    let first = space
        .service
        .query_sql(&space.space_id, query_request(base_sql(&space), 2))
        .await?;
    let continuation = first
        .next
        .clone()
        .context("first page must carry a continuation")?;

    // Changed SQL text.
    let changed_sql = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: format!(
                    "SELECT \"_ugoite_id\" FROM \"{}\" ORDER BY \"{}\"",
                    space.relation, space.status_column
                ),
                continuation: Some(continuation.clone()),
                ..query_request(base_sql(&space), 2)
            },
        )
        .await;
    assert!(
        format!("{:#}", changed_sql.expect_err("changed SQL must fail")).contains("fingerprint"),
        "changed SQL with an old continuation must fail on the fingerprint"
    );

    // Same text but a parameterised variant with different parameters.
    let parameterised = format!(
        "SELECT \"_ugoite_id\" FROM \"{}\" WHERE \"{}\" = $status ORDER BY \"_ugoite_id\"",
        space.relation, space.status_column
    );
    let param_first = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: parameterised.clone(),
                parameters: Map::from_iter([(
                    "status".to_string(),
                    Value::String("open".to_string()),
                )]),
                parameter_types: BTreeMap::from([("status".to_string(), "string".to_string())]),
                limit: 2,
                continuation: None,
                saved_sql: None,
            },
        )
        .await?;
    let param_continuation = param_first
        .next
        .clone()
        .context("parameterised first page")?;
    let changed_params = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: parameterised,
                parameters: Map::from_iter([(
                    "status".to_string(),
                    Value::String("closed".to_string()),
                )]),
                parameter_types: BTreeMap::from([("status".to_string(), "string".to_string())]),
                limit: 2,
                continuation: Some(param_continuation),
                saved_sql: None,
            },
        )
        .await;
    assert!(
        format!(
            "{:#}",
            changed_params.expect_err("changed params must fail")
        )
        .contains("fingerprint"),
        "changed parameters with an old continuation must fail on the fingerprint"
    );

    // Tampered continuation bytes fail closed.
    let mut tampered = continuation.clone();
    tampered.push('x');
    let tampered_result = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: Some(tampered),
                ..query_request(base_sql(&space), 2)
            },
        )
        .await;
    assert!(
        tampered_result.is_err(),
        "tampered continuations must fail closed"
    );
    Ok(())
}

/// `SqlQuery` is read-only and stateless: writes are rejected on both the
/// page and count paths, pagination without `ORDER BY` is rejected, and the
/// continuation token is opaque.
#[tokio::test]
async fn stateless_sql_stays_read_only_with_opaque_continuation() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-readonly", "sqlreadonly").await?;
    for sql in [
        format!("DROP TABLE \"{}\"", space.relation),
        format!(
            "INSERT INTO \"{}\" SELECT * FROM \"{}\"",
            space.relation, space.relation
        ),
        format!(
            "UPDATE \"{}\" SET \"{}\" = 'x'",
            space.relation, space.status_column
        ),
        format!("DELETE FROM \"{}\"", space.relation),
    ] {
        let page = space
            .service
            .query_sql(&space.space_id, query_request(sql.clone(), 10))
            .await;
        assert!(page.is_err(), "write SQL must be rejected: {sql}");
        let count = space
            .service
            .count_sql(
                &space.space_id,
                SqlQueryCountRequest {
                    sql,
                    parameters: Map::new(),
                    parameter_types: BTreeMap::new(),
                    saved_sql: None,
                },
            )
            .await;
        assert!(count.is_err(), "write SQL count must be rejected");
    }
    // Pagination without ORDER BY fails closed instead of returning an
    // unstable order.
    let unordered = space
        .service
        .query_sql(
            &space.space_id,
            query_request(
                format!("SELECT \"_ugoite_id\" FROM \"{}\"", space.relation),
                2,
            ),
        )
        .await;
    assert!(
        format!(
            "{:#}",
            unordered.expect_err("unordered pagination must fail")
        )
        .contains("ORDER BY"),
        "unordered pagination must demand ORDER BY"
    );

    // The continuation is an opaque signed token: versioned, and carrying no
    // SQL text, relation name, or row content.
    let first = space
        .service
        .query_sql(&space.space_id, query_request(base_sql(&space), 2))
        .await?;
    let token = first
        .next
        .clone()
        .context("first page must carry a continuation")?;
    assert!(
        token.starts_with("v1."),
        "continuation must be versioned: {token}"
    );
    assert_eq!(
        token.split('.').count(),
        3,
        "continuation must be a signed token"
    );
    assert!(
        !token.contains(&space.relation) && !token.contains("SELECT"),
        "continuation must not embed the query: {token}"
    );
    Ok(())
}

/// The continuation pins its publication: entries created after the first
/// page do not leak into the continued pages.
#[tokio::test]
async fn stateless_continuation_reads_pinned_publication() -> Result<()> {
    let space = setup_sql_space("memory://sql-stateless-pinned", "sqlpinned").await?;
    let first = space
        .service
        .query_sql(&space.space_id, query_request(base_sql(&space), 2))
        .await?;
    let continuation = first
        .next
        .clone()
        .context("first page must carry a continuation")?;

    space
        .service
        .create_structured_entry_with_receipt(
            &space.space_id,
            "sql-live",
            "Task".to_string(),
            Vec::new(),
            BTreeMap::from([
                ("Status".to_string(), Value::String("open".to_string())),
                ("Priority".to_string(), json!(0)),
            ]),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let second = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                continuation: Some(continuation),
                ..query_request(base_sql(&space), 2)
            },
        )
        .await?;
    let ids: BTreeSet<String> = second
        .rows
        .iter()
        .filter_map(|row| {
            row.get("_ugoite_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert!(
        !ids.contains("sql-live"),
        "continued page must read the pinned publication: {ids:?}"
    );
    Ok(())
}
