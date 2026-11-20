use super::{
    audit, auth_error, recovery_commit_error, recovery_fence_unavailable,
    recovery_reservation_error, recovery_storage_unavailable, space_write_outcome_is_ambiguous,
    ApiResult, Authorizer, NodeAuditInput, NodeIdentityService, RecoveryBindingSnapshot,
    SpaceOnboardingState, UgoiteService,
};
use serde_json::json;
use uuid::Uuid;

pub(super) async fn recovery_account_generations(
    identity: &NodeIdentityService,
    issuer_account_id: Uuid,
    target_account_id: Uuid,
) -> ApiResult<(u64, u64)> {
    let accounts = identity.list_accounts().await.map_err(auth_error)?;
    let issuer_generation = accounts
        .iter()
        .find(|account| account.account_id == issuer_account_id)
        .map(|account| account.credential_generation)
        .ok_or_else(recovery_fence_unavailable)?;
    let target_generation = accounts
        .iter()
        .find(|account| account.account_id == target_account_id)
        .map(|account| account.credential_generation)
        .ok_or_else(recovery_fence_unavailable)?;
    Ok((issuer_generation, target_generation))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn reserve_recovery_pair(
    identity: &NodeIdentityService,
    service: &UgoiteService,
    space_id: &str,
    space_uid: Uuid,
    issuer_principal_id: Uuid,
    issuer_account_id: Uuid,
    target_principal_id: Uuid,
    target_account_id: Uuid,
    request_id: Uuid,
    ttl: chrono::Duration,
) -> ApiResult<(
    Authorizer,
    ugoite_iceberg::authorization::RecoveryFence,
    RecoveryBindingSnapshot,
)> {
    let (issuer_generation, target_generation) =
        recovery_account_generations(identity, issuer_account_id, target_account_id).await?;
    let issuer_node_epoch = identity
        .recovery_account_lifecycle_epoch(issuer_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let target_node_epoch = identity
        .recovery_account_lifecycle_epoch(target_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let authorizer = Authorizer::new(service.operator().clone());

    // A Space CAS can commit while its response is lost, either before or
    // after the Node fence is promoted from provisional to paired. Reattach
    // the request-identified Node fence before attempting a new reservation;
    // otherwise the active Node half would make every same-key retry fail
    // closed until expiry.
    if let Some(provisional) = identity
        .recovery_fence_for_request(
            request_id,
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
        )
        .await
        .map_err(recovery_commit_error)?
    {
        let authorization = authorizer
            .state(space_id)
            .await
            .map_err(|_| recovery_fence_unavailable())?;
        let fence = match authorization
            .recovery_fences
            .get(&provisional.recovery_fence_id)
            .cloned()
        {
            Some(fence) => fence,
            None => {
                // A read which misses the Space fence is not proof that a
                // concurrent CAS is not in flight. Retry the same Space CAS
                // with the same fence identity; the Authorizer treats an
                // exact active identity as idempotent and a storage outcome
                // that remains ambiguous keeps the Node half fail-closed.
                authorizer
                    .reserve_recovery_fence_with_id(
                        space_id,
                        request_id,
                        provisional.recovery_fence_id,
                        issuer_principal_id,
                        issuer_account_id,
                        target_principal_id,
                        target_account_id,
                        issuer_generation,
                        target_generation,
                        ttl,
                    )
                    .await
                    .map_err(|_| recovery_fence_unavailable())?
            }
        };
        if fence.status != "active"
            || fence.request_id != request_id
            || fence.space_uid != space_uid
            || fence.issuer_principal_id != issuer_principal_id
            || fence.issuer_account_id != issuer_account_id
            || fence.target_principal_id != target_principal_id
            || fence.target_account_id != target_account_id
            || fence.issuer_generation != issuer_generation
            || fence.target_generation != target_generation
            || !chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                .map(|expires| expires.with_timezone(&chrono::Utc) > chrono::Utc::now())
                .unwrap_or(false)
            || !chrono::DateTime::parse_from_rfc3339(&provisional.recovery_fence_expires_at)
                .map(|expires| expires.with_timezone(&chrono::Utc) > chrono::Utc::now())
                .unwrap_or(false)
        {
            return Err(recovery_fence_unavailable());
        }
        let snapshot = RecoveryBindingSnapshot {
            request_id,
            recovery_fence_id: fence.fence_id,
            recovery_fence_expires_at: fence.expires_at.clone(),
            space_authorization_revision: fence.authorization_revision,
            issuer_space_lifecycle_epoch: fence.issuer_space_lifecycle_epoch,
            target_space_lifecycle_epoch: fence.target_space_lifecycle_epoch,
            issuer_node_lifecycle_epoch: provisional.issuer_node_lifecycle_epoch,
            target_node_lifecycle_epoch: provisional.target_node_lifecycle_epoch,
            issuer_generation,
            target_generation,
        };
        identity
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&snapshot),
            )
            .await
            .map_err(recovery_commit_error)?;
        return Ok((authorizer, fence, snapshot));
    }
    let fence_id = Uuid::now_v7();
    let recovery_fence_expires_at =
        (chrono::Utc::now() + ttl).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let provisional = RecoveryBindingSnapshot {
        request_id,
        recovery_fence_id: fence_id,
        recovery_fence_expires_at: recovery_fence_expires_at.clone(),
        space_authorization_revision: 0,
        issuer_space_lifecycle_epoch: 0,
        target_space_lifecycle_epoch: 0,
        issuer_node_lifecycle_epoch: issuer_node_epoch,
        target_node_lifecycle_epoch: target_node_epoch,
        issuer_generation,
        target_generation,
    };
    if let Err(error) = identity
        .acquire_recovery_fence(
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
            Some(&provisional),
        )
        .await
    {
        return Err(recovery_commit_error(error));
    }
    let fence = match authorizer
        .reserve_recovery_fence_with_id(
            space_id,
            request_id,
            fence_id,
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            issuer_generation,
            target_generation,
            ttl,
        )
        .await
    {
        Ok(fence) => fence,
        Err(error) => {
            if space_write_outcome_is_ambiguous(&error) {
                // The Space fence may already be durable. Keep the Node half
                // until reconciliation can inspect the paired outcome.
                return Err(recovery_fence_unavailable());
            }
            identity
                .release_recovery_fence(fence_id)
                .await
                .map_err(recovery_commit_error)?;
            return Err(recovery_reservation_error(error));
        }
    };
    let snapshot = RecoveryBindingSnapshot {
        request_id,
        recovery_fence_id: fence.fence_id,
        recovery_fence_expires_at: fence.expires_at.clone(),
        space_authorization_revision: fence.authorization_revision,
        issuer_space_lifecycle_epoch: fence.issuer_space_lifecycle_epoch,
        target_space_lifecycle_epoch: fence.target_space_lifecycle_epoch,
        issuer_node_lifecycle_epoch: issuer_node_epoch,
        target_node_lifecycle_epoch: target_node_epoch,
        issuer_generation,
        target_generation,
    };
    if let Err(error) = identity
        .acquire_recovery_fence(
            space_uid,
            target_principal_id,
            target_account_id,
            issuer_account_id,
            Some(&snapshot),
        )
        .await
    {
        identity
            .release_recovery_fence(fence.fence_id)
            .await
            .map_err(recovery_commit_error)?;
        authorizer
            .release_recovery_fence(space_id, fence.fence_id)
            .await
            .map_err(|_| recovery_storage_unavailable())?;
        return Err(recovery_commit_error(error));
    }
    Ok((authorizer, fence, snapshot))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn reserve_recovery_space_fence(
    identity: &NodeIdentityService,
    service: &UgoiteService,
    space_id: &str,
    issuer_principal_id: Uuid,
    issuer_account_id: Uuid,
    target_principal_id: Uuid,
    target_account_id: Uuid,
    request_id: Uuid,
    ttl: chrono::Duration,
) -> ApiResult<(
    Authorizer,
    ugoite_iceberg::authorization::RecoveryFence,
    RecoveryBindingSnapshot,
)> {
    let (issuer_generation, target_generation) =
        recovery_account_generations(identity, issuer_account_id, target_account_id).await?;
    let issuer_node_epoch = identity
        .recovery_account_lifecycle_epoch(issuer_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let target_node_epoch = identity
        .recovery_account_lifecycle_epoch(target_account_id)
        .await
        .map_err(recovery_commit_error)?;
    let authorizer = Authorizer::new(service.operator().clone());
    let fence = authorizer
        .reserve_recovery_fence_with_id(
            space_id,
            request_id,
            Uuid::now_v7(),
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            issuer_generation,
            target_generation,
            ttl,
        )
        .await
        .map_err(|error| {
            if space_write_outcome_is_ambiguous(&error) {
                recovery_fence_unavailable()
            } else {
                recovery_reservation_error(error)
            }
        })?;
    let snapshot = RecoveryBindingSnapshot {
        request_id,
        recovery_fence_id: fence.fence_id,
        recovery_fence_expires_at: fence.expires_at.clone(),
        space_authorization_revision: fence.authorization_revision,
        issuer_space_lifecycle_epoch: fence.issuer_space_lifecycle_epoch,
        target_space_lifecycle_epoch: fence.target_space_lifecycle_epoch,
        issuer_node_lifecycle_epoch: issuer_node_epoch,
        target_node_lifecycle_epoch: target_node_epoch,
        issuer_generation,
        target_generation,
    };
    Ok((authorizer, fence, snapshot))
}

pub(super) async fn reconcile_recovery_fences(
    identity: &NodeIdentityService,
    service: &UgoiteService,
    space_id: &str,
) -> anyhow::Result<()> {
    let space_uid = service.space_uid(space_id).await?;
    let authorizer = Authorizer::new(service.operator().clone());
    let pending_ids = identity.pending_recovery_fence_ids(space_uid).await?;
    for fence_id in pending_ids.iter().copied() {
        let fence = match authorizer.recovery_fence(space_id, fence_id).await {
            Ok(fence) => fence,
            Err(_) => continue,
        };
        let space_fence_ready = match fence.status.as_str() {
            "active" => {
                let expires_at = chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                    .map(|expires| expires.with_timezone(&chrono::Utc))
                    .map_err(|_| anyhow::anyhow!("invalid stored recovery fence timestamp"))?;
                if expires_at <= chrono::Utc::now() {
                    // The Node mutation is durable, but the Space half never
                    // committed. Release both halves as one terminal
                    // reconciliation; expiry is not a Node-only release.
                    authorizer
                        .release_recovery_fence(space_id, fence_id)
                        .await?;
                    identity
                        .abort_recovery_fence_after_space_abort(fence_id)
                        .await?;
                    continue;
                }
                authorizer
                    .complete_recovery_fence(space_id, fence_id)
                    .await
                    .is_ok()
            }
            "completed" => true,
            // A Node mutation is durable once it appears in pending_ids. A
            // released Space fence is nevertheless a safe terminal outcome:
            // recovery changes Node credentials only, never Space membership.
            "released" => {
                identity
                    .abort_recovery_fence_after_space_abort(fence_id)
                    .await?;
                continue;
            }
            _ => false,
        };
        if !space_fence_ready {
            continue;
        }
        if identity.complete_recovery_fence(fence_id).await.is_err() {
            if identity
                .expired_recovery_fence(fence_id)
                .await
                .unwrap_or(false)
            {
                identity
                    .abort_recovery_fence_after_space_abort(fence_id)
                    .await?;
            }
            continue;
        }
        identity.mark_recovery_fence_reconciled(fence_id).await?;
    }

    // A force-reset request has a server-generated request ID, so a caller
    // cannot retry an ambiguous Space CAS by itself. Replay every durable
    // provisional Node reservation during reconciliation; the exact fence and
    // request identities make this safe whether the original Space write
    // committed or not.
    for provisional in identity
        .active_provisional_recovery_fences(space_uid)
        .await?
    {
        let _ = reserve_recovery_pair(
            identity,
            service,
            space_id,
            space_uid,
            provisional.issuer_principal_id,
            provisional.issuer_account_id,
            provisional.target_principal_id,
            provisional.target_account_id,
            provisional.snapshot.request_id,
            chrono::Duration::minutes(15),
        )
        .await;
    }
    let authorization = authorizer.state(space_id).await?;
    let now = chrono::Utc::now();
    let mut expired_space_fences = Vec::new();
    for fence in authorization.recovery_fences.values() {
        if fence.status != "active" || pending_ids.contains(&fence.fence_id) {
            continue;
        }
        let expires_at = chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
            .map(|expires| expires.with_timezone(&chrono::Utc))
            .map_err(|_| anyhow::anyhow!("invalid stored recovery fence timestamp"))?;
        if expires_at <= now {
            expired_space_fences.push(fence.fence_id);
        }
    }
    for fence_id in expired_space_fences {
        // An expired approval without a Node commit is explicitly aborted
        // here. Expiry alone never releases a write barrier.
        identity.release_recovery_fence(fence_id).await?;
        authorizer
            .release_recovery_fence(space_id, fence_id)
            .await?;
    }
    // A supersession may commit the Node-side invalidation before the paired
    // Space release. On restart, the terminal Node status is enough to finish
    // that release; an active Node status remains fail-closed.
    let current_authorization = authorizer.state(space_id).await?;
    for fence in current_authorization
        .recovery_fences
        .values()
        .filter(|fence| fence.status == "active" && !pending_ids.contains(&fence.fence_id))
    {
        if matches!(
            identity
                .recovery_fence_status(fence.fence_id)
                .await?
                .as_deref(),
            Some("released" | "superseded" | "completed")
        ) {
            authorizer
                .release_recovery_fence(space_id, fence.fence_id)
                .await?;
        }
    }
    for fence_id in identity.active_recovery_fence_ids(space_uid).await? {
        if pending_ids.contains(&fence_id) {
            continue;
        }
        let phase = identity.recovery_fence_phase(fence_id).await?;
        let Some(space_fence) = current_authorization.recovery_fences.get(&fence_id) else {
            // A read which misses the Space fence cannot prove that a CAS is
            // not in flight in another process. Keep both the provisional and
            // paired Node barriers fail-closed; the same-key retry replays the
            // exact Space fence identity and can converge once that CAS is
            // observable. Releasing here could let a delayed Space write
            // publish a fence with no Node barrier.
            continue;
        };
        if matches!(space_fence.status.as_str(), "released" | "completed") {
            identity.release_recovery_fence(fence_id).await?;
            continue;
        }
        if phase.as_deref() != Some("provisional")
            && identity.expired_recovery_fence(fence_id).await?
        {
            // Do not release a paired Node fence based on its local clock
            // alone; its Space half must already be terminal.
            continue;
        }
    }
    Ok(())
}

pub(super) async fn reconcile_all_recovery_fences(
    identity: &NodeIdentityService,
    service: &UgoiteService,
) -> anyhow::Result<()> {
    for space_id in service.list_space_ids().await? {
        match service
            .classify_space_for_node_onboarding(&space_id)
            .await?
        {
            SpaceOnboardingState::ClaimedValid { .. } => {
                reconcile_recovery_fences(identity, service, &space_id).await?;
            }
            // A portable Space added after Node setup has no authorization
            // state by design. Keep it quarantined while allowing unrelated
            // authorized Spaces to recover normally.
            SpaceOnboardingState::PortableUnclaimed { .. } => {}
            SpaceOnboardingState::IncompleteOrPending { uid, slug, reason }
            | SpaceOnboardingState::InvalidOrForeign { uid, slug, reason } => {
                anyhow::bail!(
                    "Space onboarding validation failed during recovery (uid={uid:?}, slug={slug:?}): {reason}"
                );
            }
        }
    }
    Ok(())
}

pub(super) async fn abort_owner_recovery_fence(
    identity: &NodeIdentityService,
    service: &UgoiteService,
    space_uid: Uuid,
    fence_id: Uuid,
) -> ApiResult<()> {
    let space_id = find_space_id_by_uid(service, space_uid)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    identity
        .release_recovery_fence(fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Authorizer::new(service.operator().clone())
        .release_recovery_fence(&space_id, fence_id)
        .await
        .map_err(|_| recovery_storage_unavailable())?;
    Ok(())
}

pub(super) async fn reconcile_recovery_audit_outbox(
    identity: &NodeIdentityService,
    service: &UgoiteService,
    space_id: &str,
) -> anyhow::Result<()> {
    let space_uid = service.space_uid(space_id).await?;
    for record in identity
        .pending_recovery_audits()
        .await?
        .into_iter()
        .filter(|record| record.space_uid == space_uid)
    {
        if record.status == "pending" {
            identity
                .append_node_audit_with_id(
                    record.event_id,
                    NodeAuditInput {
                        subject_account_id: Some(record.account_id),
                        actor_account_id: record.actor_account_id,
                        credential_id: record.actor_credential_id.or(record.credential_id),
                        action: &record.action,
                        target_type: "space_principal",
                        target_id: Some(record.principal_id.to_string()),
                        outcome: "success",
                        request_id: Some(record.request_id.to_string()),
                        safe_metadata: record
                            .event
                            .get("metadata")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    },
                )
                .await?;
            identity
                .mark_recovery_audit_stage(record.event_id, "node")
                .await?;
        }
        let record = identity
            .pending_recovery_audits()
            .await?
            .into_iter()
            .find(|candidate| candidate.event_id == record.event_id);
        let Some(record) = record else { continue };
        if record.status == "node" {
            audit::append_audit_event(service.operator(), space_id, &record.event, None).await?;
            identity
                .mark_recovery_audit_stage(record.event_id, "space")
                .await?;
        }
        if identity
            .pending_recovery_audits()
            .await?
            .into_iter()
            .any(|candidate| candidate.event_id == record.event_id && candidate.status == "space")
        {
            identity
                .mark_recovery_audit_delivered(record.event_id)
                .await?;
        }
    }
    Ok(())
}

async fn find_space_id_by_uid(service: &UgoiteService, space_uid: Uuid) -> anyhow::Result<String> {
    for space_id in service.list_space_ids().await? {
        if service.space_uid(&space_id).await? == space_uid {
            return Ok(space_id);
        }
    }
    anyhow::bail!("invited Space is not present on this node")
}
