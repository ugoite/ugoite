use anyhow::Result;
use chrono::Duration;
use serde_json::{json, Value};
use ugoite_domain::identity::{PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole};
use ugoite_iceberg::authorization::Authorizer;
use ugoite_storage::operator_from_uri;
use uuid::Uuid;

#[test]
fn owner_recovery_contract_exposes_owner_only_space_scope() {
    let snapshot = ugoite_server::openapi_snapshot();
    let force_reset = snapshot
        .pointer("/paths/~1spaces~1{space_id}~1admin~1recovery~1force-reset/post")
        .expect("owner force-reset endpoint");
    assert!(force_reset["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|parameter| parameter["$ref"] == "#/components/parameters/SpaceId"));
    assert_eq!(force_reset["parameters"].as_array().unwrap().len(), 1);
    assert!(force_reset["responses"].get("422").is_none());
    assert_eq!(
        force_reset["responses"]["201"]["headers"]["Cache-Control"]["schema"]["const"],
        "no-store"
    );
}

#[test]
fn owner_recovery_contract_exposes_space_binding_semantics() {
    let snapshot = ugoite_server::openapi_snapshot();
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1start/post")
        .is_some());
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post")
        .is_some());
}

#[test]
fn owner_recovery_contract_exposes_single_reset_response() {
    let snapshot = ugoite_server::openapi_snapshot();
    let response = snapshot
        .pointer("/components/schemas/OwnerRecoveryFinishResponse")
        .expect("owner recovery response schema");
    assert_eq!(
        response["required"],
        serde_json::json!(["account", "recovery_codes", "audit_status"])
    );
}

#[test]
fn owner_recovery_contract_exposes_audit_status() {
    let snapshot = ugoite_server::openapi_snapshot();
    let status = snapshot
        .pointer("/components/schemas/AuditStatus")
        .expect("audit status schema");
    assert_eq!(status["enum"], serde_json::json!(["delivered", "pending"]));
    let approval = snapshot
        .pointer("/components/schemas/OwnerRecoveryApprovalResponse")
        .expect("approval response schema");
    assert!(approval["properties"].get("owner_approval_token").is_some());
    assert!(approval["properties"].get("token_hash").is_none());
}

#[tokio::test]
async fn test_req_sec_013_recovery_audit_replay_is_idempotent_on_filesystem_storage() {
    let root = tempfile::tempdir().expect("temporary audit root");
    let first_operator =
        ugoite_storage::operator_from_uri(&format!("fs://{}", root.path().display()))
            .expect("first filesystem operator");
    let second_operator =
        ugoite_storage::operator_from_uri(&format!("fs://{}", root.path().display()))
            .expect("second filesystem operator");
    let event_id = uuid::Uuid::new_v4();
    let credential_id = uuid::Uuid::new_v4();
    let payload = json!({
        "event_id": event_id,
        "action": "recovery.space_binding_replaced",
        "subject_principal_id": uuid::Uuid::new_v4(),
        "actor_principal_id": uuid::Uuid::new_v4(),
        "actor_account_id": uuid::Uuid::new_v4(),
        "credential_id": credential_id,
        "metadata": {"space_uid": uuid::Uuid::new_v4(), "old_account_id": uuid::Uuid::new_v4(), "new_account_id": uuid::Uuid::new_v4(), "recovery_request_id": event_id}
    });
    let (first, replay) = tokio::join!(
        ugoite_iceberg::audit::append_audit_event(&first_operator, "demo", &payload, None),
        ugoite_iceberg::audit::append_audit_event(&second_operator, "demo", &payload, None),
    );
    let first = first.expect("first audit append");
    let replay = replay.expect("replayed audit append");
    assert_eq!(first["event_id"], event_id.to_string());
    assert_eq!(replay["event_id"], event_id.to_string());
    assert_eq!(first["credential_id"], credential_id.to_string());
    assert_eq!(replay["credential_id"], credential_id.to_string());
    let listing = ugoite_iceberg::audit::list_audit_events(
        &second_operator,
        "demo",
        ugoite_iceberg::audit::AuditListOptions::default(),
    )
    .await
    .expect("audit listing");
    assert_eq!(listing["total"], 1);
}

#[test]
fn owner_recovery_contract_exposes_terminal_error_and_cookie_contract() {
    let snapshot: Value = ugoite_server::openapi_snapshot();
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post/responses/201/headers/Set-Cookie")
        .is_some());
    let error_codes = snapshot
        .pointer("/components/schemas/RecoveryErrorResponse/properties/code/enum")
        .and_then(Value::as_array)
        .expect("recovery error code enum");
    assert!(error_codes
        .iter()
        .any(|code| code == "OWNER_APPROVAL_EXPIRED"));
    let code = "OWNER_APPROVAL_ALREADY_COMMITTED";
    assert!(
        error_codes.iter().any(|value| value == code),
        "missing {code}"
    );
    let credential_required = snapshot
        .pointer("/components/schemas/WebAuthnRegistrationCredential/required")
        .and_then(Value::as_array)
        .expect("registration credential required fields");
    assert!(!credential_required
        .iter()
        .any(|field| field == "extensions"));
}

#[test]
fn test_req_sec_012_returns_a_fresh_account_contract() {
    let snapshot = ugoite_server::openapi_snapshot();
    let response = snapshot
        .pointer("/components/schemas/OwnerRecoveryFinishResponse")
        .expect("owner recovery finish response");
    assert_eq!(
        response["required"],
        json!(["account", "recovery_codes", "audit_status"])
    );
    assert_eq!(
        snapshot["components"]["schemas"]["AuditStatus"]["enum"],
        json!(["delivered", "pending"])
    );
}

#[test]
fn test_req_sec_012_concurrent_completion_has_terminal_error_contract() {
    let snapshot = ugoite_server::openapi_snapshot();
    let errors = snapshot["components"]["schemas"]["RecoveryErrorResponse"]["properties"]["code"]
        ["enum"]
        .as_array()
        .expect("recovery error codes");
    assert!(errors
        .iter()
        .any(|code| code == "SPACE_RECOVERY_ALREADY_COMPLETED"));
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post/responses/201/headers/Set-Cookie")
        .is_some());
}

#[tokio::test]
async fn test_req_sec_012_owner_only_space_scope_and_target_binding() -> Result<()> {
    let operator = operator_from_uri("memory://owner-recovery-behavior")?;
    operator.create_dir("spaces/demo/").await?;
    let authorizer = Authorizer::new(operator);
    let owner = Uuid::now_v7();
    let target = Uuid::now_v7();
    let issuer_account = Uuid::now_v7();
    let target_account = Uuid::now_v7();
    let space_uid = Uuid::now_v7();
    authorizer
        .initialize_owner("demo", space_uid, owner, "Owner")
        .await?;
    authorizer
        .add_human_member(
            "demo",
            owner,
            SpacePrincipal {
                principal_id: target,
                kind: PrincipalKind::Human,
                display_name: "Target".to_string(),
                state: PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;
    let non_owner = Uuid::now_v7();
    authorizer
        .add_human_member(
            "demo",
            owner,
            SpacePrincipal {
                principal_id: non_owner,
                kind: PrincipalKind::Human,
                display_name: "Non-owner".to_string(),
                state: PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;
    assert!(authorizer
        .reserve_recovery_fence(
            "demo",
            Uuid::now_v7(),
            non_owner,
            Uuid::now_v7(),
            target,
            target_account,
            0,
            0,
            Duration::minutes(5),
        )
        .await
        .is_err());

    let fence = authorizer
        .reserve_recovery_fence(
            "demo",
            Uuid::now_v7(),
            owner,
            issuer_account,
            target,
            target_account,
            0,
            0,
            Duration::minutes(5),
        )
        .await?;
    assert!(authorizer
        .change_role("demo", owner, target, SpaceRole::Editor)
        .await
        .is_err());
    assert!(authorizer
        .reserve_recovery_fence(
            "demo",
            Uuid::now_v7(),
            owner,
            issuer_account,
            target,
            target_account,
            0,
            0,
            Duration::minutes(5),
        )
        .await
        .is_err());
    authorizer
        .release_recovery_fence("demo", fence.fence_id)
        .await?;

    let expired = authorizer
        .reserve_recovery_fence(
            "demo",
            Uuid::now_v7(),
            owner,
            issuer_account,
            target,
            target_account,
            0,
            0,
            Duration::seconds(-1),
        )
        .await?;
    assert!(authorizer
        .complete_recovery_fence("demo", expired.fence_id)
        .await
        .is_err());
    Ok(())
}
