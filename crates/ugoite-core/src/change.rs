//! Semantic Change orchestration shared by REST, MCP, and future clients.
//!
//! This module intentionally stops before storage. An adapter receives the
//! validated request-scoped context and performs exactly one authoritative
//! prepare/publication operation.

pub use ugoite_domain::change::{
    selective_inverse, selective_inverse_with_form_schema, selective_inverse_with_schema,
    ChangeCommand, ChangeDescriptor, Conflict, MutationContext, RevertPlan, RunId,
};

/// Validate a client-supplied command against the authenticated principal and
/// produce the narrow context that resource adapters are allowed to consume.
pub fn apply_change_context(
    command: &ChangeCommand,
    authenticated_actor: &str,
    request_id: Option<String>,
) -> Result<MutationContext, ugoite_domain::change::ChangeValidationError> {
    command.into_context(authenticated_actor, request_id)
}

/// Return Change descriptors in the order used by history-only Run Undo.
/// Durable progress is represented by the resulting reachable publications,
/// not by a Run status record.
pub fn undo_order<'a>(
    changes: impl IntoIterator<Item = &'a ChangeDescriptor>,
) -> Vec<&'a ChangeDescriptor> {
    let mut changes = changes.into_iter().collect::<Vec<_>>();
    changes.sort_by(|left, right| right.created_at_micros.cmp(&left.created_at_micros));
    changes
}
