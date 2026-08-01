mod common;

use chrono::{Duration, Utc};
use common::setup_operator;
use std::collections::BTreeSet;
use std::collections::{BTreeMap, HashSet};
use ugoite_core::error::{AppError, ErrorKind};
use ugoite_domain::identity::{
    AccessPolicy, Action, AgentMode, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
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
            form.to_string(),
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

fn assert_forbidden(error: &anyhow::Error) {
    assert_eq!(
        error.downcast_ref::<AppError>().map(AppError::kind),
        Some(ErrorKind::Forbidden)
    );
}

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
    let entry_relation = form::get_form(&op, ws_path, "Entry").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();

    let entry_one = "---\nform: Entry\n---\n# Alpha\n\n## Body\nalpha";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    let entry_two = "---\nform: Entry\n---\n# Beta\n\n## Body\nbeta";
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;

    let sql_payload = saved_sql::SqlPayload {
        name: "Alpha Query".to_string(),
        sql: format!(
            "SELECT * FROM \"{entry_relation}\" WHERE _ugoite_title = $title ORDER BY _ugoite_id"
        ),
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
        authorized_entries(&entry_relation, &["entry-1", "entry-2"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
    };
    let create_authorization = sql_session::SqlSessionCreateAuthorization {
        authorization,
        readable_entries_by_form: &readable_entries_by_form,
    };

    let no_principals = [];
    let error = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{entry_relation}\" ORDER BY _ugoite_id"),
        sql_session::SqlSessionCreateAuthorization {
            authorization: sql_session::SqlSessionAuthorization {
                principal_ids: &no_principals,
                policy_hash: AUTHORIZATION_POLICY_HASH,
            },
            readable_entries_by_form: &readable_entries_by_form,
        },
    )
    .await
    .expect_err("a public SQL-session constructor must reject an empty principal set");
    assert_forbidden(&error);

    let oversized_entries = (0..=ugoite_iceberg::index::SQL_SESSION_MAX_AUTHORIZATION_SCOPE_IDS)
        .map(|index| format!("entry-{index}"))
        .collect::<HashSet<_>>();
    let oversized_scope = [(entry_relation.clone(), oversized_entries)]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let error = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{entry_relation}\" ORDER BY _ugoite_id"),
        sql_session::SqlSessionCreateAuthorization {
            authorization,
            readable_entries_by_form: &oversized_scope,
        },
    )
    .await
    .expect_err("an explicit authorization scope must be bounded before checkpoint creation");
    assert!(error
        .to_string()
        .contains("authorization scope exceeds the configured maximum"));

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
            create_authorization,
        )
        .await?;
    assert_eq!(session["status"], "ready");
    assert_eq!(session["parameters"], serde_json::json!({"title": "Alpha"}));
    assert_eq!(
        session["parameter_types"],
        serde_json::json!({"title": "string"})
    );
    let session_id = session["id"].as_str().unwrap();
    let query_policy = serde_json::from_value(session["query_policy"].clone())?;
    let execution_authorization = sql_session::SqlSessionExecutionAuthorization {
        authorization,
        query_policy: &query_policy,
    };

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
        execution_authorization,
    )
    .await?;
    assert_eq!(count, 1);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        execution_authorization,
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
    let public_task_relation = form::get_form(&op, ws_path, "PublicTask").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
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
        authorized_entries(&public_task_relation, &["public-a", "public-b"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
    };
    let create_authorization = sql_session::SqlSessionCreateAuthorization {
        authorization,
        readable_entries_by_form: &readable_entries_by_form,
    };
    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{public_task_relation}\" ORDER BY _ugoite_id DESC LIMIT 2"),
        create_authorization,
    )
    .await?;
    let session_id = session["id"].as_str().unwrap();
    let query_policy = serde_json::from_value(session["query_policy"].clone())?;
    let execution_authorization = sql_session::SqlSessionExecutionAuthorization {
        authorization,
        query_policy: &query_policy,
    };

    let count = sql_session::get_sql_session_count_authorized_by_form(
        &op,
        ws_path,
        session_id,
        execution_authorization,
    )
    .await?;
    assert_eq!(count, 2);

    let rows = sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        execution_authorization,
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
    let task_relation = form::get_form(&op, ws_path, "Task").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
    entry::create_entry(
        &op,
        ws_path,
        "task-1",
        "---\nform: Task\n---\n# Task one\n\n## Summary\nOne\n",
        "author",
        &MockIntegrity,
    )
    .await?;
    let (principal_id, readable_entries_by_form) = authorized_entries(&task_relation, &["task-1"]);
    let principal_ids = [principal_id];
    let authorization = sql_session::SqlSessionAuthorization {
        principal_ids: &principal_ids,
        policy_hash: AUTHORIZATION_POLICY_HASH,
    };
    let create_authorization = sql_session::SqlSessionCreateAuthorization {
        authorization,
        readable_entries_by_form: &readable_entries_by_form,
    };

    for sql in [
        format!("SELECT * FROM \"{task_relation}\""),
        format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_updated_at"),
        format!("SELECT DISTINCT _ugoite_id FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        format!("SELECT _ugoite_title AS _ugoite_id FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        format!("SELECT * FROM \"{task_relation}\" WHERE EXISTS (SELECT 1 FROM \"{task_relation}\" t2 WHERE t2._ugoite_id = \"{task_relation}\"._ugoite_id) ORDER BY _ugoite_id"),
        format!("SELECT (SELECT _ugoite_id FROM \"{task_relation}\" LIMIT 1) FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        format!("SELECT * FROM \"{task_relation}\" WHERE _ugoite_id IN (SELECT _ugoite_id FROM \"{task_relation}\") ORDER BY _ugoite_id"),
        format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id LIMIT 1 OFFSET 1000000"),
    ] {
        assert!(
            sql_session::create_sql_session_authorized_for_principals_by_form(
                &op,
                ws_path,
                &sql,
                create_authorization,
            )
            .await
            .is_err()
        );
    }

    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        create_authorization,
    )
    .await?;
    let session_id = session["id"].as_str().expect("session id");
    let query_policy = serde_json::from_value(session["query_policy"].clone())?;
    let execution_authorization = sql_session::SqlSessionExecutionAuthorization {
        authorization,
        query_policy: &query_policy,
    };
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        execution_authorization,
        1_000,
        1,
    )
    .await
    .is_err());
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        execution_authorization,
        usize::MAX,
        1,
    )
    .await
    .is_err());
    assert!(sql_session::get_sql_session_rows_authorized_by_form(
        &op,
        ws_path,
        session_id,
        sql_session::SqlSessionExecutionAuthorization {
            authorization: sql_session::SqlSessionAuthorization {
                policy_hash: "sha256:changed-policy",
                ..authorization
            },
            query_policy: &query_policy,
        },
        0,
        1,
    )
    .await
    .is_err());

    let limit_zero = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id LIMIT 0"),
        create_authorization,
    )
    .await?;
    let limit_zero_id = limit_zero["id"].as_str().expect("session id");
    let limit_zero_query_policy = serde_json::from_value(limit_zero["query_policy"].clone())?;
    let limit_zero_execution_authorization = sql_session::SqlSessionExecutionAuthorization {
        authorization,
        query_policy: &limit_zero_query_policy,
    };
    assert_eq!(
        sql_session::get_sql_session_count_authorized_by_form(
            &op,
            ws_path,
            limit_zero_id,
            limit_zero_execution_authorization,
        )
        .await?,
        0
    );
    assert_eq!(
        sql_session::get_sql_session_rows_authorized_by_form(
            &op,
            ws_path,
            limit_zero_id,
            limit_zero_execution_authorization,
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
        limit_zero_execution_authorization,
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
    let task_relation = service.get_form(&space_id, "Task").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
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
            &format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        )
        .await?;
    let session_id = session["id"].as_str().expect("session ID");
    assert_eq!(
        session["query_policy"]["forms"][0]["entry_scope"],
        serde_json::json!({"all_except": []})
    );
    assert!(session["query_policy"]["forms"][0]
        .get("entry_ids")
        .is_none());

    let no_principals = [];
    let error = service
        .create_sql_session_authorized_for_principals(
            &space_id,
            &no_principals,
            &format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        )
        .await
        .expect_err("session creation must reject an empty principal set");
    assert_forbidden(&error);
    let error = service
        .get_sql_session_authorized_for_principals(&space_id, session_id, &no_principals)
        .await
        .expect_err("session status must reject an empty principal set");
    assert_forbidden(&error);
    let error = service
        .get_sql_session_count_authorized_for_principals(&space_id, session_id, &no_principals)
        .await
        .expect_err("session count must reject an empty principal set");
    assert_forbidden(&error);
    let error = service
        .get_sql_session_rows_authorized_for_principals(&space_id, session_id, &no_principals, 0, 1)
        .await
        .expect_err("session rows must reject an empty principal set");
    assert_forbidden(&error);

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

#[tokio::test]
async fn sql_sessions_apply_sparse_entry_denials_in_the_provider() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op, "memory://sql-session-sparse-scope");
    let owner = Uuid::from_u128(81);
    let viewer = Uuid::from_u128(82);
    let space_id = service
        .create_space_for_principal("sql-session-sparse-scope", owner, "Owner")
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
    let task_relation = service.get_form(&space_id, "Task").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
    for (id, title) in [("task-public", "Public"), ("task-private", "Private")] {
        service
            .create_entry(
                &space_id,
                id,
                &format!("---\nform: Task\n---\n# {title}\n\n## Summary\n{title}\n"),
                "owner",
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
                display_name: "Viewer".to_string(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
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
                id: "task-private".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;

    let principals = [viewer];
    let session = service
        .create_sql_session_authorized_for_principals(
            &space_id,
            &principals,
            &format!("SELECT * FROM \"{task_relation}\" ORDER BY _ugoite_id"),
        )
        .await?;
    assert_eq!(
        session["query_policy"]["forms"][0]["entry_scope"],
        serde_json::json!({"all_except": ["task-private"]})
    );
    let session_id = session["id"].as_str().expect("session ID");
    assert_eq!(
        service
            .get_sql_session_count_authorized_for_principals(&space_id, session_id, &principals)
            .await?,
        1
    );
    assert_eq!(
        service
            .get_sql_session_rows_authorized_for_principals(
                &space_id,
                session_id,
                &principals,
                0,
                1,
            )
            .await?["rows"][0]["_ugoite_id"],
        "task-public"
    );

    // Query policy is durable derived metadata, not execution authority. Each
    // access must reject a policy that no longer equals the scope, Form, and
    // projection reconstructed from the immutable checkpoint and current ACL.
    let meta_path = format!(
        "{}/sql_sessions/{session_id}/meta.json",
        service.workspace_path(&space_id)
    );
    let original_meta: serde_json::Value =
        serde_json::from_slice(&service.operator().read(&meta_path).await?.to_vec())?;
    let original_policy = original_meta["query_policy"].clone();
    let mut all_current = original_policy.clone();
    all_current["forms"][0]["entry_scope"] = serde_json::json!("all_current");
    let mut empty_all_except = original_policy.clone();
    empty_all_except["forms"][0]["entry_scope"] = serde_json::json!({"all_except": []});
    let mut extra_system_columns = original_policy.clone();
    extra_system_columns["forms"][0]["system_columns"] = serde_json::json!([
        "external_id",
        "title",
        "created_at",
        "updated_at",
        "entry_id",
        "entry_version",
        "committed_at",
    ]);
    let mut different_form = original_policy.clone();
    different_form["forms"][0]["form_id"] = serde_json::json!(Uuid::now_v7().to_string());
    let mut different_relation = original_policy.clone();
    different_relation["forms"][0]["relation"] = serde_json::json!("other");
    let mut different_columns = original_policy.clone();
    different_columns["forms"][0]["columns"] = serde_json::json!(["other_column"]);
    let oversized_scope = (0..=ugoite_iceberg::index::SQL_SESSION_MAX_AUTHORIZATION_SCOPE_IDS)
        .map(|index| format!("injected-{index}"))
        .collect::<Vec<_>>();
    let mut oversized_scope_policy = original_policy.clone();
    oversized_scope_policy["forms"][0]["entry_scope"] =
        serde_json::json!({"all_except": oversized_scope});

    for (name, query_policy) in [
        ("all_current scope", all_current),
        ("empty all_except scope", empty_all_except),
        ("extra system columns", extra_system_columns),
        ("different Form ID", different_form),
        ("different relation", different_relation),
        ("different columns", different_columns),
        ("oversized scope", oversized_scope_policy),
    ] {
        let mut meta = original_meta.clone();
        meta["query_policy"] = query_policy;
        service
            .operator()
            .write(&meta_path, serde_json::to_vec(&meta)?)
            .await?;

        let error = service
            .get_sql_session_authorized_for_principals(&space_id, session_id, &principals)
            .await
            .expect_err(&format!("status must reject a tampered {name}"));
        assert_forbidden(&error);
        let error = service
            .get_sql_session_count_authorized_for_principals(&space_id, session_id, &principals)
            .await
            .expect_err(&format!("count must reject a tampered {name}"));
        assert_forbidden(&error);
        let error = service
            .get_sql_session_rows_authorized_for_principals(
                &space_id,
                session_id,
                &principals,
                0,
                1,
            )
            .await
            .expect_err(&format!("rows must reject a tampered {name}"));
        assert_forbidden(&error);
    }
    service
        .operator()
        .write(&meta_path, serde_json::to_vec(&original_meta)?)
        .await?;

    Ok(())
}

#[tokio::test]
async fn sql_session_rejects_unsupported_sql_before_resolving_a_space_scope() -> anyhow::Result<()>
{
    let service = UgoiteService::from_operator(
        setup_operator()?,
        "memory://sql-session-early-shape-validation",
    );
    let principals = [Uuid::from_u128(91)];
    let error = service
        .create_sql_session_authorized_for_principals(
            "missing-space",
            &principals,
            "SELECT * FROM task JOIN other ON task._ugoite_id = other._ugoite_id ORDER BY task._ugoite_id",
        )
        .await
        .expect_err("unsupported SQL must fail before the Space is opened");
    assert!(error.to_string().contains("does not support joins"));
    Ok(())
}

#[tokio::test]
async fn sql_session_uses_backend_relation_mapping_for_hyphenated_forms() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-session-relation", "/tmp").await?;
    let ws_path = "spaces/test-sql-session-relation";
    let form_def = serde_json::json!({
        "name": "Daily-Note",
        "template": "# Daily-Note\n\n## Count\n",
        "fields": {
            "Count": {"type": "integer"},
            "Enabled": {"type": "boolean"}
        }
    });
    form::upsert_form(&op, ws_path, &form_def).await?;
    let relation = form::get_form(&op, ws_path, "Daily-Note").await?["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
    let count_column = form::get_form(&op, ws_path, "Daily-Note").await?["fields"]["Count"]
        ["sql_column"]
        .as_str()
        .expect("Form SQL column")
        .to_string();
    let enabled_column = form::get_form(&op, ws_path, "Daily-Note").await?["fields"]["Enabled"]
        ["sql_column"]
        .as_str()
        .expect("Form SQL column")
        .to_string();

    let principal_ids = [Uuid::from_u128(92)];
    let readable_entries_by_form = [(relation.clone(), HashSet::new())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let session = sql_session::create_sql_session_authorized_for_principals_by_form(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id"),
        sql_session::SqlSessionCreateAuthorization {
            authorization: sql_session::SqlSessionAuthorization {
                principal_ids: &principal_ids,
                policy_hash: AUTHORIZATION_POLICY_HASH,
            },
            readable_entries_by_form: &readable_entries_by_form,
        },
    )
    .await?;
    assert_eq!(session["status"], "ready");
    assert_eq!(session["query_policy"]["forms"][0]["relation"], relation);

    let parameters = serde_json::Map::from_iter([
        ("search_0".to_string(), serde_json::json!(10)),
        (
            "search_1".to_string(),
            serde_json::json!("2025-03-04T00:00:00.000Z"),
        ),
        ("search_2".to_string(), serde_json::json!(true)),
    ]);
    let parameter_types = BTreeMap::from_iter([
        ("search_0".to_string(), "integer".to_string()),
        ("search_1".to_string(), "timestamp".to_string()),
        ("search_2".to_string(), "boolean".to_string()),
    ]);
    let typed_session = sql_session::create_sql_session_authorized_for_principals_by_form_with_parameters(
        &op,
        ws_path,
        &format!("SELECT * FROM \"{relation}\" WHERE _ugoite_updated_at < $search_1 AND \"{count_column}\" = $search_0 AND \"{enabled_column}\" = $search_2 ORDER BY _ugoite_updated_at DESC, _ugoite_id"),
        parameters,
        parameter_types,
        sql_session::SqlSessionCreateAuthorization {
            authorization: sql_session::SqlSessionAuthorization {
                principal_ids: &principal_ids,
                policy_hash: AUTHORIZATION_POLICY_HASH,
            },
            readable_entries_by_form: &readable_entries_by_form,
        },
    )
    .await?;
    assert_eq!(typed_session["status"], "ready");
    Ok(())
}

#[tokio::test]
async fn sql_relations_are_unique_for_case_distinct_forms() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "case-distinct-forms", "/tmp").await?;
    let ws_path = "spaces/case-distinct-forms";
    for name in ["Meeting", "meeting"] {
        form::upsert_form(
            &op,
            ws_path,
            &serde_json::json!({"name": name, "fields": {}}),
        )
        .await?;
    }
    let upper = form::get_form(&op, ws_path, "Meeting").await?;
    let lower = form::get_form(&op, ws_path, "meeting").await?;
    let upper_relation = upper["sql_relation"].as_str().expect("SQL relation");
    let lower_relation = lower["sql_relation"].as_str().expect("SQL relation");
    assert_ne!(upper_relation, lower_relation);

    let principal_ids = [Uuid::from_u128(93)];
    let readable_entries_by_form = [
        (upper_relation.to_string(), HashSet::new()),
        (lower_relation.to_string(), HashSet::new()),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    for relation in [upper_relation, lower_relation] {
        let session = sql_session::create_sql_session_authorized_for_principals_by_form(
            &op,
            ws_path,
            &format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id"),
            sql_session::SqlSessionCreateAuthorization {
                authorization: sql_session::SqlSessionAuthorization {
                    principal_ids: &principal_ids,
                    policy_hash: AUTHORIZATION_POLICY_HASH,
                },
                readable_entries_by_form: &readable_entries_by_form,
            },
        )
        .await?;
        assert_eq!(session["status"], "ready");
    }
    Ok(())
}
