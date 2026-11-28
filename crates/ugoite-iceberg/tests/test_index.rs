mod common;
use common::legacy_entry as entry;
use common::setup_operator;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::{form, index, space};

#[tokio::test]
/// REQ-SRCH-007
async fn test_index_req_idx_001_reindex_writes_index_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    index::reindex_all(&op, ws_path).await?;

    // AssetText is a derived Iceberg relation; legacy ad-hoc index files are
    // not created.
    assert!(!op.exists(&format!("{}/index/index.json", ws_path)).await?);
    assert!(
        op.exists(&format!(
            "{}/_ugoite/derived/relations/{}/head.json",
            ws_path,
            ugoite_domain::derived_relation::DerivedRelationId::ASSET_TEXT
        ))
        .await?
    );

    Ok(())
}

#[tokio::test]
/// REQ-SRCH-007
async fn test_index_req_idx_004_inverted_index_generation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-ws", "/tmp").await?;
    let ws_path = "spaces/test-ws";

    index::reindex_all(&op, ws_path).await?;
    assert!(
        !op.exists(&format!("{}/index/inverted_index.json", ws_path))
            .await?
    );
    Ok(())
}

#[test]
/// REQ-SRCH-007
fn test_index_req_idx_005_word_count() {
    let content = "One two three";
    let count = index::compute_word_count(content);
    assert_eq!(count, 3);
}

#[test]
fn sql_template_normalization_ignores_literals_identifiers_and_comments() -> anyhow::Result<()> {
    let sql = "SELECT '{{literal}}', \"{{identifier}}\", $native -- {{comment}}\nFROM table WHERE id = {{actual}}";
    assert_eq!(
        index::normalize_sql_template(sql)?,
        "SELECT '{{literal}}', \"{{identifier}}\", $native -- {{comment}}\nFROM table WHERE id = $actual"
    );
    assert_eq!(
        index::normalize_sql_template("SELECT $one, {{bad-name}}, {{two}}")
            .unwrap_err()
            .to_string(),
        "SQL template variable name must match [A-Za-z_][A-Za-z0-9_]*"
    );
    assert_eq!(
        index::normalize_sql_template("SELECT {{missing")
            .unwrap_err()
            .to_string(),
        "SQL template variable is missing closing braces"
    );
    assert!(index::validate_sql_syntax("SELECTE broken").is_err());
    assert!(index::validate_sql_syntax("SELEC broken").is_err());
    assert!(index::validate_sql_syntax("").is_err());
    assert!(index::validate_sql_syntax("SELECT 1").is_ok());
    // Syntax lint deliberately does not enforce the separate read-only
    // execution policy. The authorized query context rejects this statement.
    assert!(index::validate_sql_syntax("DROP TABLE entries").is_ok());
    for sql in [
        "INSERT INTO entries SELECT * FROM entries",
        "UPDATE entries SET field_100 = 'changed'",
        "DELETE FROM entries",
    ] {
        assert!(
            index::validate_sql_syntax(sql).is_ok(),
            "parser-valid DML must remain accepted by syntax lint: {sql}"
        );
    }
    Ok(())
}

#[tokio::test]
/// REQ-SRCH-007
async fn test_index_req_idx_001_get_space_stats() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "stats-space", "/tmp").await?;
    let ws_path = "spaces/stats-space";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }
        fn signature(&self, _data: &str) -> String {
            "mock-sig".to_string()
        }
    }

    let class_def = serde_json::json!({
        "name": "Note",
        "fields": {"Body": {"type": "markdown"}}
    });
    form::upsert_form(&op, ws_path, &class_def).await?;
    entry::create_entry(
        &op,
        ws_path,
        "note-1",
        "---\nform: Note\n---\n# Note 1\n\n## Body\nHello",
        "author",
        &MockIntegrity,
    )
    .await?;

    let stats = index::get_space_stats(&op, ws_path).await?;
    assert!(stats.is_object());

    Ok(())
}

#[tokio::test]
async fn space_stats_count_non_null_typed_values_without_partial_results() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "stats-null-field", "/tmp").await?;
    let ws_path = "spaces/stats-null-field";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Note",
            "fields": {"Body": {"type": "markdown"}},
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;
    entry::create_entry(
        &op,
        ws_path,
        "with-body",
        "---\nform: Note\n---\n# With body\n\n## Body\nvalue",
        "author",
        &integrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "without-body",
        "---\nform: Note\n---\n# Without body",
        "author",
        &integrity,
    )
    .await?;

    let stats = index::get_space_stats(&op, ws_path).await?;
    assert_eq!(stats["entry_count"], 2);
    assert_eq!(stats["form_stats"]["Note"]["count"], 2);
    assert_eq!(stats["form_stats"]["Note"]["fields"]["Body"], 1);
    Ok(())
}

#[tokio::test]
async fn space_stats_aggregates_tags_across_forms_in_datafusion() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "stats-cross-form-tags", "/tmp").await?;
    let ws_path = "spaces/stats-cross-form-tags";
    for form_name in ["Alpha", "Beta"] {
        form::upsert_form(
            &op,
            ws_path,
            &serde_json::json!({
                "name": form_name,
                "fields": {"Body": {"type": "markdown"}},
            }),
        )
        .await?;
    }
    let integrity = FakeIntegrityProvider;
    entry::create_entry(
        &op,
        ws_path,
        "alpha-1",
        "---\nform: Alpha\ntags: [shared, alpha]\n---\n# Alpha\n\n## Body\nvalue",
        "author",
        &integrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "beta-1",
        "---\nform: Beta\ntags: [shared, beta]\n---\n# Beta\n\n## Body\nvalue",
        "author",
        &integrity,
    )
    .await?;

    let stats = index::get_space_stats(&op, ws_path).await?;
    assert_eq!(stats["tag_counts"]["shared"], 2);
    assert_eq!(stats["tag_counts"]["alpha"], 1);
    assert_eq!(stats["tag_counts"]["beta"], 1);
    Ok(())
}
