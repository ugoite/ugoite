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
//! - declared parameter types filter, inferred `long`/`double` numbers bind,
//!   and typed nulls bind without substitution;
//! - reusing a continuation after the SQL text, parameters, or types change
//!   resets (fails closed) instead of reading the old coordinate;
//! - write statements stay read-only and the continuation stays opaque.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryCountRequest, SqlQueryRequest};
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
async fn legacy_id_relation_saved_sql_revision_runs_without_rewrite() -> Result<()> {
    let space = setup_sql_space("memory://sql-legacy-saved-relation", "sqllegacysaved").await?;
    let legacy_sql = base_sql(&space);
    let saved = space
        .service
        .create_saved_sql(
            &space.space_id,
            Some("legacy-id-query"),
            &SqlPayload {
                name: Some("Legacy ID query".into()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: legacy_sql.clone(),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    assert_eq!(saved["metadata"]["formBindings"], json!([]));
    let revision_id = saved["revision_id"].as_str().context("revision id")?;
    let page = space
        .service
        .query_sql(
            &space.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                limit: 10,
                continuation: None,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: "legacy-id-query".into(),
                    revision_id: revision_id.into(),
                }),
            },
        )
        .await?;
    assert_eq!(page.rows.len(), 5);
    let after = space
        .service
        .get_saved_sql(&space.space_id, "legacy-id-query")
        .await?;
    assert_eq!(after["revision_id"], revision_id);
    assert_eq!(after["sql"], legacy_sql);
    assert_eq!(after["metadata"]["formBindings"], json!([]));
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
