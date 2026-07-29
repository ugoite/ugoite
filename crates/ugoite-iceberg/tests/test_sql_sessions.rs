mod common;

use chrono::{Duration, Utc};
use common::setup_operator;
use std::collections::BTreeSet;
use std::collections::{BTreeMap, HashSet};
use ugoite_domain::identity::{AccessPolicy, Action, AgentMode};
use ugoite_iceberg::{
    authorization::{Authorizer, CreateAgentRequest, ResourceKind, ResourceRef},
    entry, form, saved_sql,
    service::UgoiteService,
    space, sql_session,
};
use uuid::Uuid;

fn authorized_entries(form: &str, entry_ids: &[&str]) -> (Uuid, BTreeMap<String, HashSet<String>>) {
    (
        Uuid::from_u128(1),
        [(
            form.to_ascii_lowercase(),
            entry_ids
                .iter()
                .map(|entry_id| (*entry_id).to_string())
                .collect(),
        )]
        .into_iter()
        .collect(),
    )
}

const AUTHORIZATION_POLICY_HASH: &str = "sha256:test-authorization-policy";

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_end_to_end() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session", "/tmp").await?;
    let ws_path = "spaces/test-sql-session";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}}
    });
    form::upsert_form(&op, ws_path, &form_def).await?;

    let entry_one = "---\nform: Entry\n---\n# Alpha\n\n## Body\nalpha";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    let entry_two = "---\nform: Entry\n---\n# Beta\n\n## Body\nbeta";
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;

    let sql_payload = saved_sql::SqlPayload {
        name: "Alpha Query".to_string(),
        sql: "SELECT * FROM entry WHERE _ugoite_title = $title ORDER BY _ugoite_id".to_string(),
        variables: serde_json::json!([{
            "name": "title",
            "type": "string",
            "description": "Entry title",
        }]),
    };
    saved_sql::create_sql(
        &op,
        ws_path,
        "sql-alpha",
        &sql_payload,
        "author",
        &MockIntegrity,
    )
    .await?;

    let (principal_id, readable_entries_by_form) =
        authorized_entries("Entry", &["entry-1", "entry-2"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };
    let parameters = [("title".to_string(), serde_json::json!("Alpha"))]
        .into_iter()
        .collect();
    let parameter_types = [("title".to_string(), "string".to_string())]
        .into_iter()
        .collect();
    let session =
        sql_session::create_sql_session_authorized_for_principals_by_form_with_parameters(
            &op,
            ws_path,
            &sql_payload.sql,
            parameters,
            parameter_types,
            authorization,
        )
        .await?;
    assert_eq!(session["status"], "ready");
    assert_eq!(session["parameters"], serde_json::json!({"title": "Alpha"}));
    assert_eq!(
        session["parameter_types"],
        serde_json::json!({"title": "string"})
    );
    let session_id = session["id"].as_str().unwrap();

    entry::create_entry(
        &op,
        ws_path,
        "entry-3",
        "---\nform: Entry\n---\n# After checkpoint\n\n## Body\nnew",
        "author",
        &MockIntegrity,
    )
    .await?;

    let count = sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
    )
    .await?;
    assert_eq!(count, 1);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        0,
        10,
    )
    .await?;
    assert_eq!(rows["total_count"], 1);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 1);
    assert_eq!(rows_list[0]["_ugoite_id"], "entry-1");

    Ok(())
}

#[tokio::test]
/// REQ-API-008
async fn test_sql_sessions_req_api_008_scopes_rows_before_limit() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session-acl", "/tmp").await?;
    let ws_path = "spaces/test-sql-session-acl";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "PublicTask",
            "template": "# PublicTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "RestrictedTask",
            "template": "# RestrictedTask\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;

    entry::create_entry(
        &op,
        ws_path,
        "public-a",
        "---\nform: PublicTask\n---\n# Public A\n\n## Summary\nPublic A\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "public-b",
        "---\nform: PublicTask\n---\n# Public B\n\n## Summary\nPublic B\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "restricted-z",
        "---\nform: RestrictedTask\n---\n# Restricted Z\n\n## Summary\nRestricted Z\n",
        "author",
        &MockIntegrity,
    )
    .await?;

    let (principal_id, readable_entries_by_form) =
        authorized_entries("PublicTask", &["public-a", "public-b"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };
    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        "SELECT * FROM publictask ORDER BY _ugoite_id DESC LIMIT 2",
        authorization,
    )
    .await?;
    let session_id = session["id"].as_str().unwrap();

    let count = sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
    )
    .await?;
    assert_eq!(count, 2);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        0,
        10,
    )
    .await?;
    assert_eq!(rows["total_count"], 2);
    let rows_list = rows["rows"].as_array().unwrap();
    assert_eq!(rows_list.len(), 2);
    assert_eq!(rows_list[0]["_ugoite_id"], "public-b");
    assert_eq!(rows_list[1]["_ugoite_id"], "public-a");

    Ok(())
}

#[tokio::test]
async fn sql_sessions_reject_unsafe_pagination_and_authorization_changes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session-boundaries", "/tmp").await?;
    let ws_path = "spaces/test-sql-session-boundaries";

    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "template": "# Task\n\n## Summary\n",
            "fields": {"Summary": {"type": "string"}},
        }),
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "task-1",
        "---\nform: Task\n---\n# Task one\n\n## Summary\nOne\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    let (principal_id, readable_entries_by_form) = authorized_entries("Task", &["task-1"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
        readable_entries_by_form: &readable_entries_by_form,
    };

    for sql in [
        "SELECT * FROM task",
        "SELECT * FROM task ORDER BY _ugoite_updated_at",
        "SELECT DISTINCT _ugoite_id FROM task ORDER BY _ugoite_id",
        "SELECT _ugoite_title AS _ugoite_id FROM task ORDER BY _ugoite_id",
        "SELECT * FROM task WHERE EXISTS (SELECT 1 FROM task t2 WHERE t2._ugoite_id = task._ugoite_id) ORDER BY _ugoite_id",
        "SELECT (SELECT _ugoite_id FROM task LIMIT 1) FROM task ORDER BY _ugoite_id",
        "SELECT * FROM task WHERE _ugoite_id IN (SELECT _ugoite_id FROM task) ORDER BY _ugoite_id",
        "SELECT * FROM task ORDER BY _ugoite_id LIMIT 1 OFFSET 1000000",
    ] {
        assert!(
            sql_session::create_sql_session_authorized_for_principals_by_form(
                &op,
                ws_path,
                sql,
                authorization,
            )
            .await
            .is_err()
        );
    }

    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        "SELECT * FROM task ORDER BY _ugoite_id",
        authorization,
    )
    .await?;
    let session_id = session["id"].as_str().expect("session id");
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        1_000,
        1,
    )
    .await
    .is_err());
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        authorization,
        usize::MAX,
        1,
    )
    .await
    .is_err());
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        sql_session::SqlSessionAuthorization {
            policy_hash: "sha256:changed-policy",
            ..authorization
        },
        0,
        1,
    )
    .await
    .is_err());

    let limit_zero = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        "SELECT * FROM task ORDER BY _ugoite_id LIMIT 0",
        authorization,
    )
    .await?;
    let limit_zero_id = limit_zero["id"].as_str().expect("session id");
    assert_eq!(
        sql_session::get_sql_session_count_authorized_by_form(
            &op,
            ws_path,
            limit_zero_id,
            authorization,
        )
        .await?,
        0
    );
    assert_eq!(
        sql_session::get_sql_session_rows_authorized_by_form(
            &op,
            ws_path,
            limit_zero_id,
            authorization,
            0,
            1,
        )
        .await?["rows"],
        serde_json::json!([])
    );

    let meta_path = format!("{ws_path}/sql_sessions/{limit_zero_id}/meta.json");
    let mut meta: serde_json::Value = serde_json::from_slice(&op.read(&meta_path).await?.to_vec())?;
    meta["authorized_principal_ids"] = serde_json::json!(["not-a-uuid"]);
    op.write(&meta_path, serde_json::to_vec(&meta)?).await?;
    assert!(sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        limit_zero_id,
        authorization,
    )
    .await
    .is_err());

    Ok(())
}

#[tokio::test]
/// REQ-API-008: production service calls retain a frozen checkpoint policy.
async fn sql_sessions_service_freezes_checkpoint_scope_and_policy() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op, "memory://sql-session-service");
    let owner = Uuid::from_u128(44);
    let space_id = service
        .create_space_for_principal("sql-session-service", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &serde_json::json!({
                "name": "Task",
                "template": "# Task\n\n## Summary\n",
                "fields": {"Summary": {"type": "string"}},
            }),
        )
        .await?;
    for (id, title) in [("task-1", "One"), ("task-2", "Two")] {
        service
            .create_entry(
                &space_id,
                id,
                &format!("---\nform: Task\n---\n# {title}\n\n## Summary\n{title}\n"),
                "owner",
            )
            .await?;
    }

    // `last_used_at` advances the legacy global authorization revision but
    // does not affect the owner's effective policy or this session scope.
    let authorizer = Authorizer::new(service.operator().clone());
    let agent = authorizer
        .create_agent(
            &space_id,
            owner,
            CreateAgentRequest {
                display_name: "Unrelated reader".to_string(),
                description: String::new(),
                mode: AgentMode::Autonomous,
                owner_principal_ids: [owner].into_iter().collect::<BTreeSet<_>>(),
                granted_actions: [Action::Read].into_iter().collect(),
                expires_at: Some((Utc::now() + Duration::hours(1)).to_rfc3339()),
            },
        )
        .await?;

    let principals = [owner];
    let session = service
        .create_sql_session_authorized_for_principals(
            &space_id,
            &principals,
            "SELECT * FROM task ORDER BY _ugoite_id",
        )
        .await?;
    let session_id = session["id"].as_str().expect("session ID");

    service.delete_entry(&space_id, "task-1", false).await?;
    service
        .create_entry(
            &space_id,
            "task-3",
            "---\nform: Task\n---\n# Three\n\n## Summary\nThree\n",
            "owner",
        )
        .await?;
    service
        .upsert_form(
            &space_id,
            &serde_json::json!({
                "name": "Task",
                "template": "# Task\n\n## Summary\n\n## Detail\n",
                "fields": {
                    "Summary": {"type": "string"},
                    "Detail": {"id": 101, "type": "string"},
                },
            }),
        )
        .await?;

    authorizer
        .mark_agent_used(&space_id, agent.agent_id)
        .await?;
    assert_eq!(
        service
            .get_sql_session_authorized_for_principals(&space_id, session_id, &principals)
            .await?["status"],
        "ready"
    );
    assert_eq!(
        service
            .get_sql_session_count_authorized_for_principals(&space_id, session_id, &principals)
            .await?,
        2
    );
    let rows = service
        .get_sql_session_rows_authorized_for_principals(&space_id, session_id, &principals, 0, 2)
        .await?;
    assert_eq!(rows["total_count"], 2);
    assert_eq!(
        rows["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["_ugoite_id"].as_str().expect("entry ID"))
            .collect::<Vec<_>>(),
        vec!["task-1", "task-2"]
    );

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "task-1".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    assert!(service
        .get_sql_session_authorized_for_principals(&space_id, session_id, &principals)
        .await
        .is_err());
    assert!(service
        .get_sql_session_count_authorized_for_principals(&space_id, session_id, &principals)
        .await
        .is_err());
    assert!(service
        .get_sql_session_rows_authorized_for_principals(&space_id, session_id, &principals, 0, 1)
        .await
        .is_err());

    Ok(())
}
