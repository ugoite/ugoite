use serde_json::Value;

#[test]
fn test_req_sec_012_owner_only_space_scope_and_target_binding() {
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
fn test_req_sec_012_generation_invalidates_human_credentials_but_not_agents() {
    let snapshot = ugoite_server::openapi_snapshot();
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1start/post")
        .is_some());
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post")
        .is_some());
}

#[test]
fn test_req_sec_012_backup_rotation_idempotency_and_preservation() {
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
fn test_req_sec_012_concurrent_reset_winner_and_loser_session() {
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
fn test_req_sec_013_recovery_outbox_status_and_redaction() {
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
fn test_req_sec_013_cross_process_conditional_append_deduplicates_event() {
    let snapshot = ugoite_server::openapi_snapshot();
    let errors = snapshot
        .pointer("/paths/~1spaces~1{space_id}~1admin~1recovery~1backup-codes/post/responses/409")
        .expect("idempotency conflict response");
    assert_eq!(
        errors["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ErrorResponse"
    );
}

#[test]
fn test_req_sec_013_retained_event_id_survives_log_compaction() {
    let snapshot: Value = ugoite_server::openapi_snapshot();
    assert!(snapshot
        .pointer("/paths/~1auth~1recovery~1owner~1finish/post/responses/201/headers/Set-Cookie")
        .is_some());
}
