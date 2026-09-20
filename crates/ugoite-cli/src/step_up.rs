//! CLI→browser step-up handoff for remote Space mutations.
//!
//! Issue #2511: a remote CLI speaks with device tokens that can never carry
//! a recent-Passkey ceremony, so Space mutations gated on fresh human
//! presence fail with `RECENT_PASSKEY_REQUIRED` and no way forward. This
//! module completes the journey without weakening the policy:
//!
//! `remote mutation → RECENT_PASSKEY_REQUIRED → browser step-up → same CLI
//! intent retried once with a single-use challenge`
//!
//! The CLI never mints credentials, never reuses human-approval tokens, and
//! never bypasses the ceremony: the browser approves a short-lived
//! account/credential/operation/Space-bound challenge after its own fresh
//! Passkey ceremony, the server consumes it once, and authorization is
//! re-evaluated. Non-TTY callers get a deterministic machine-readable
//! `STEP_UP_REQUIRED` error instead of an interactive prompt.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::time::{Duration, Instant};
use ugoite_api_client::ApiProtocolError;

use crate::http;

/// Connection-bound step-up handoff for Space-bound remote mutations.
///
/// Same journey as [`execute_with_step_up`], but every leg (initial intent,
/// `auth.step_up.start`, `auth.step_up.status` polling, and the single retry)
/// resolves credentials through the [`crate::cli_config::SpaceTarget`]
/// connection boundary: a context-first remote uses exactly its named
/// credential profile with no implicit global fallback.
pub async fn execute_with_step_up_for_target(
    target: &crate::cli_config::SpaceTarget,
    operation: &str,
    arguments: Value,
    body: Option<Value>,
    space_id: Option<&str>,
) -> Result<Value> {
    match http::execute_for_target(target, operation, arguments.clone(), body.clone()).await {
        Ok(value) => return Ok(value),
        Err(error) if !recent_passkey_required(&error) => return Err(error),
        Err(_) => {}
    }
    let started = http::execute_for_target(
        target,
        "auth.step_up.start",
        json!({}),
        Some(json!({"operation": operation, "space_id": space_id})),
    )
    .await
    .context("start step-up challenge for the remote mutation")?;
    let challenge_id = step_up_string(&started["challenge_id"], "challenge_id")?;
    let verification_uri = step_up_string(&started["verification_uri"], "verification_uri")?;
    // The server returns both `verification_uri` and
    // `verification_uri_complete`; prefer the complete handoff URI and fall
    // back to the plain URI when the server omits it.
    let verification_uri_complete = started["verification_uri_complete"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&verification_uri)
        .to_string();
    let expires_in = started["expires_in"].as_u64().unwrap_or(600);
    let interval = started["interval"].as_u64().unwrap_or(5).clamp(1, 60);

    if !std::io::stdout().is_terminal() {
        eprintln!(
            "{}",
            json!({
                "code": "STEP_UP_REQUIRED",
                "operation": operation,
                "verification_uri": verification_uri,
                "verification_uri_complete": verification_uri_complete,
                "expires_in": expires_in,
                "challenge_id": challenge_id,
            })
        );
        bail!(
            "remote mutation requires a fresh Passkey ceremony (STEP_UP_REQUIRED): open {verification_uri_complete} in a signed-in browser within {expires_in}s, approve the step-up, then retry"
        );
    }

    eprintln!("Remote mutation needs a fresh Passkey ceremony to continue.");
    eprintln!("Open {verification_uri_complete} in a signed-in browser and approve the step-up,");
    eprintln!("then return here: this command retries the identical mutation once.");
    let deadline = Instant::now() + Duration::from_secs(expires_in.min(600));
    loop {
        if Instant::now() >= deadline {
            bail!("step-up challenge expired before browser approval");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        // Unknown, expired, and consumed challenges fail closed with
        // 403 STEP_UP_INVALID (no 404 branch): polling ends terminally and
        // the caller starts a new challenge for a retry.
        let status = match http::execute_for_target(
            target,
            "auth.step_up.status",
            json!({"challenge_id": challenge_id}),
            None,
        )
        .await
        {
            Ok(status) => status,
            Err(error) if step_up_invalid(&error) => {
                bail!("step-up challenge is no longer available")
            }
            Err(error) => return Err(error).context("check step-up challenge status"),
        };
        match status["status"].as_str().unwrap_or_default() {
            "approved" => break,
            "pending" => continue,
            "expired" => bail!("step-up challenge expired before browser approval"),
            "consumed" => bail!("step-up challenge was already consumed"),
            _ => bail!("step-up challenge is no longer available"),
        }
    }
    let mut retry_arguments = arguments;
    if let Some(object) = retry_arguments.as_object_mut() {
        object.insert("step_up".to_string(), Value::String(challenge_id));
    }
    http::execute_for_target(target, operation, retry_arguments, body).await
}

/// Reports whether a remote failure is the fresh-ceremony gate (and only
/// that gate). Every other error, including human-approval denials, passes
/// through untouched: step-up and human approval are distinct concepts.
pub fn recent_passkey_required(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ApiProtocolError>()
        .and_then(|protocol| protocol.payload.as_deref())
        .and_then(|payload| payload.get("code"))
        .and_then(Value::as_str)
        == Some("RECENT_PASSKEY_REQUIRED")
}

/// Reports whether a remote failure is the converged fail-closed step-up
/// code (unknown, expired, consumed, or mismatched challenge).
fn step_up_invalid(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ApiProtocolError>()
        .and_then(|protocol| protocol.payload.as_deref())
        .and_then(|payload| payload.get("code"))
        .and_then(Value::as_str)
        == Some("STEP_UP_INVALID")
}

fn step_up_string(field: &Value, name: &str) -> Result<String> {
    field
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .with_context(|| format!("step-up challenge response is missing {name}"))
}

/// Executes a remote Space mutation, completing the browser step-up handoff
/// when the server requires fresh human presence.
///
/// Legacy explicit-Space transport: resolves credentials through the 0.1.x
/// global session lookup. Space-bound callers with a resolved
/// [`crate::cli_config::SpaceTarget`] must use
/// [`execute_with_step_up_for_target`] instead so context-first remotes stay
/// on the connection-bound credential boundary (no implicit global fallback).
///
/// TTY callers get guidance plus polling and one automatic retry of the
/// identical intent with the approved challenge. Non-TTY callers get a
/// deterministic `STEP_UP_REQUIRED` error carrying the verification URI and
/// expiry instead of an interactive prompt.
pub async fn execute_with_step_up(
    base_url: &str,
    operation: &str,
    arguments: Value,
    body: Option<Value>,
    space_id: Option<&str>,
) -> Result<Value> {
    match http::execute(base_url, operation, arguments.clone(), body.clone()).await {
        Ok(value) => return Ok(value),
        Err(error) if !recent_passkey_required(&error) => return Err(error),
        Err(_) => {}
    }
    let started = http::execute(
        base_url,
        "auth.step_up.start",
        json!({}),
        Some(json!({"operation": operation, "space_id": space_id})),
    )
    .await
    .context("start step-up challenge for the remote mutation")?;
    let challenge_id = step_up_string(&started["challenge_id"], "challenge_id")?;
    let verification_uri = step_up_string(&started["verification_uri"], "verification_uri")?;
    // The server returns both `verification_uri` and
    // `verification_uri_complete`; prefer the complete handoff URI and fall
    // back to the plain URI when the server omits it.
    let verification_uri_complete = started["verification_uri_complete"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&verification_uri)
        .to_string();
    let expires_in = started["expires_in"].as_u64().unwrap_or(600);
    let interval = started["interval"].as_u64().unwrap_or(5).clamp(1, 60);

    if !std::io::stdout().is_terminal() {
        eprintln!(
            "{}",
            json!({
                "code": "STEP_UP_REQUIRED",
                "operation": operation,
                "verification_uri": verification_uri,
                "verification_uri_complete": verification_uri_complete,
                "expires_in": expires_in,
                "challenge_id": challenge_id,
            })
        );
        bail!(
            "remote mutation requires a fresh Passkey ceremony (STEP_UP_REQUIRED): open {verification_uri_complete} in a signed-in browser within {expires_in}s, approve the step-up, then retry"
        );
    }

    eprintln!("Remote mutation needs a fresh Passkey ceremony to continue.");
    eprintln!("Open {verification_uri_complete} in a signed-in browser and approve the step-up,");
    eprintln!("then return here: this command retries the identical mutation once.");
    let deadline = Instant::now() + Duration::from_secs(expires_in.min(600));
    loop {
        if Instant::now() >= deadline {
            bail!("step-up challenge expired before browser approval");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        // Unknown, expired, and consumed challenges fail closed with
        // 403 STEP_UP_INVALID (no 404 branch): polling ends terminally and
        // the caller starts a new challenge for a retry.
        let status = match http::execute(
            base_url,
            "auth.step_up.status",
            json!({"challenge_id": challenge_id}),
            None,
        )
        .await
        {
            Ok(status) => status,
            Err(error) if step_up_invalid(&error) => {
                bail!("step-up challenge is no longer available")
            }
            Err(error) => return Err(error).context("check step-up challenge status"),
        };
        match status["status"].as_str().unwrap_or_default() {
            "approved" => break,
            "pending" => continue,
            "expired" => bail!("step-up challenge expired before browser approval"),
            "consumed" => bail!("step-up challenge was already consumed"),
            _ => bail!("step-up challenge is no longer available"),
        }
    }
    let mut retry_arguments = arguments;
    if let Some(object) = retry_arguments.as_object_mut() {
        object.insert("step_up".to_string(), Value::String(challenge_id));
    }
    http::execute(base_url, operation, retry_arguments, body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recent_passkey_error() -> anyhow::Error {
        decode_error(json!({
            "code": "RECENT_PASSKEY_REQUIRED",
            "message": "repeat Passkey authentication within five minutes"
        }))
    }

    fn decode_error(body: Value) -> anyhow::Error {
        ugoite_api_client::decode_response(
            "space.create",
            ugoite_api_client::ApiResponse {
                status: 403,
                status_text: "Forbidden".to_string(),
                headers: vec![],
                body: body.to_string(),
            },
        )
        .expect_err("must fail")
        .into()
    }

    #[test]
    fn detects_only_the_fresh_ceremony_gate() {
        assert!(recent_passkey_required(&recent_passkey_error()));
        assert!(!recent_passkey_required(&decode_error(json!({
            "code": "HUMAN_APPROVAL_REQUIRED",
            "message": "dangerous"
        }))));
        assert!(!recent_passkey_required(&decode_error(json!({
            "code": "STEP_UP_INVALID",
            "message": "nope"
        }))));
        assert!(!recent_passkey_required(&anyhow::anyhow!(
            "plain transport error"
        )));
    }

    #[test]
    fn rejects_challenge_responses_missing_handoff_fields() {
        assert!(step_up_string(&json!(null), "verification_uri").is_err());
        assert!(step_up_string(&json!("  "), "verification_uri").is_err());
        assert_eq!(
            step_up_string(
                &json!("https://node/step-up?challenge=1"),
                "verification_uri"
            )
            .expect("uri"),
            "https://node/step-up?challenge=1"
        );
    }
}
