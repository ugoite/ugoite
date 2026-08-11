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
    assert_eq!(
        force_reset["responses"]["201"]["headers"]["Cache-Control"]["schema"]["const"],
        "no-store"
    );
}

#[test]
fn owner_recovery_contract_exposes_generation_and_agent_semantics() {
    let snapshot = ugoite_server::openapi_snapshot();
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1start/post")
        .is_some());
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post")
        .is_some());
}

#[test]
fn owner_recovery_contract_exposes_backup_rotation_idempotency() {
    let snapshot = ugoite_server::openapi_snapshot();
    let backup = snapshot
        .pointer("/paths/~1spaces~1{space_id}~1admin~1recovery~1backup-codes/post")
        .expect("backup-code rotation endpoint");
    assert_eq!(backup["parameters"][1]["name"], "Idempotency-Key");
    assert_eq!(backup["parameters"][1]["required"], true);
    assert_eq!(
        backup["responses"]["200"]["headers"]["Cache-Control"]["schema"]["const"],
        "no-store"
    );
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
fn owner_recovery_contract_exposes_audit_status_and_redaction() {
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

#[test]
fn recovery_contract_exposes_cross_process_deduplication_response() {
    let snapshot = ugoite_server::openapi_snapshot();
    let errors = snapshot
        .pointer("/paths/~1spaces~1{space_id}~1admin~1recovery~1backup-codes/post/responses/409")
        .expect("idempotency conflict response");
    assert_eq!(
        errors["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/RecoveryErrorResponse"
    );
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
        "action": "recovery.owner_reset_completed",
        "subject_principal_id": uuid::Uuid::new_v4(),
        "actor_principal_id": uuid::Uuid::new_v4(),
        "actor_account_id": uuid::Uuid::new_v4(),
        "credential_id": credential_id,
        "metadata": {"credential_generation": 2}
    });
    let first = ugoite_iceberg::audit::append_audit_event(&first_operator, "demo", &payload, None)
        .await
        .expect("first audit append");
    let replay =
        ugoite_iceberg::audit::append_audit_event(&second_operator, "demo", &payload, None)
            .await
            .expect("replayed audit append");
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
    let credential_required = snapshot
        .pointer("/components/schemas/WebAuthnRegistrationCredential/required")
        .and_then(Value::as_array)
        .expect("registration credential required fields");
    assert!(!credential_required
        .iter()
        .any(|field| field == "extensions"));
}

#[tokio::test]
async fn test_req_sec_012_behavioral_owner_fence_blocks_mutations_and_expired_completion(
) -> Result<()> {
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
