//! Domain values for reversible Knowledge changes.
//!
//! A Change is semantic metadata attached to an immutable publication. This
//! module intentionally contains no storage state machine, transaction log, or
//! retry/receipt persistence.

use crate::entry::FieldValue;
use crate::form::{FieldType, FormField, ListItemDefinition};
use crate::id::FieldId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const MAX_CHANGE_ID_BYTES: usize = 128;
pub const MAX_CHANGE_MESSAGE_BYTES: usize = 4 * 1024;

/// Correlation only. A Run has no durable status of its own.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(String);

impl RunId {
    pub fn new(value: impl Into<String>) -> Result<Self, ChangeValidationError> {
        let value = value.into();
        validate_text(&value, "run_id", MAX_CHANGE_ID_BYTES)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The semantic metadata stored with one Knowledge publication.
///
/// `actor_principal_id` is historical, opaque provenance. It is not a
/// credential, grant, or Node-local account authority, so a portable Space may
/// retain an identifier that is not resolvable on another Node.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeDescriptor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub actor_principal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverts_change_id: Option<String>,
    pub created_at_micros: i64,
}

impl ChangeDescriptor {
    pub fn validate(&self) -> Result<(), ChangeValidationError> {
        validate_text(
            &self.actor_principal_id,
            "actor_principal_id",
            MAX_CHANGE_ID_BYTES,
        )?;
        if let Some(message) = &self.message {
            validate_text(message, "message", MAX_CHANGE_MESSAGE_BYTES)?;
        }
        if let Some(change_id) = &self.reverts_change_id {
            validate_text(change_id, "reverts_change_id", MAX_CHANGE_ID_BYTES)?;
        }
        Ok(())
    }
}

/// Input to a single semantic mutation. `change_id` is also the immutable
/// publication command identity; no second Knowledge identity is created.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeCommand {
    pub change_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub actor_principal_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverts_change_id: Option<String>,
    pub created_at_micros: i64,
}

/// Request-scoped context passed from Core to one authoritative resource
/// adapter. `request_id` is ephemeral and never becomes publication state.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MutationContext {
    pub change_id: String,
    pub run_id: Option<RunId>,
    pub actor_principal_id: String,
    pub request_id: Option<String>,
}

impl ChangeCommand {
    pub fn validate(&self) -> Result<(), ChangeValidationError> {
        validate_text(&self.change_id, "change_id", MAX_CHANGE_ID_BYTES)?;
        self.descriptor().validate()
    }

    pub fn descriptor(&self) -> ChangeDescriptor {
        ChangeDescriptor {
            run_id: self.run_id.clone(),
            actor_principal_id: self.actor_principal_id.clone(),
            message: self.message.clone(),
            reverts_change_id: self.reverts_change_id.clone(),
            created_at_micros: self.created_at_micros,
        }
    }

    pub fn into_context(
        &self,
        authenticated_actor: &str,
        request_id: Option<String>,
    ) -> Result<MutationContext, ChangeValidationError> {
        self.validate()?;
        if self.actor_principal_id != authenticated_actor {
            return Err(ChangeValidationError {
                field: "actor_principal_id",
                reason: "does not match authenticated actor",
                kind: ChangeValidationErrorKind::ActorMismatch,
            });
        }
        Ok(MutationContext {
            change_id: self.change_id.clone(),
            run_id: self.run_id.clone(),
            actor_principal_id: authenticated_actor.to_owned(),
            request_id,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Conflict {
    FieldChanged { field_id: FieldId },
    FieldDeleted { field_id: FieldId },
    IncompatibleType { field_id: FieldId },
    FormRemoved,
    NotRevertible { reason: String },
}

impl fmt::Display for Conflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldChanged { field_id } => {
                write!(
                    formatter,
                    "field {} changed after the target Change",
                    field_id.get()
                )
            }
            Self::FieldDeleted { field_id } => {
                write!(
                    formatter,
                    "field {} no longer exists in the current schema",
                    field_id.get()
                )
            }
            Self::IncompatibleType { field_id } => write!(
                formatter,
                "historical value for field {} is incompatible with the current schema",
                field_id.get()
            ),
            Self::FormRemoved => formatter.write_str("the target Form is removed"),
            Self::NotRevertible { reason } => {
                write!(formatter, "change is not revertible: {reason}")
            }
        }
    }
}

impl std::error::Error for Conflict {}

/// The result of comparing a target Change's before/after values with the
/// current values. `Restore` may contain `None` when the target added a field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum RevertFieldAction {
    Keep,
    Restore { value: Option<FieldValue> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RevertPlan {
    pub reverts_change_id: String,
    pub fields: BTreeMap<FieldId, RevertFieldAction>,
}

/// Compute a selective inverse without rewinding the Catalog Head.
pub fn selective_inverse(
    reverts_change_id: impl Into<String>,
    before: &BTreeMap<FieldId, FieldValue>,
    after: &BTreeMap<FieldId, FieldValue>,
    current: &BTreeMap<FieldId, FieldValue>,
) -> Result<RevertPlan, Conflict> {
    let mut field_ids = BTreeSet::new();
    field_ids.extend(before.keys().copied());
    field_ids.extend(after.keys().copied());
    field_ids.extend(current.keys().copied());

    let fields = field_ids
        .into_iter()
        .map(|field_id| {
            let before_value = before.get(&field_id);
            let after_value = after.get(&field_id);
            let current_value = current.get(&field_id);
            let action = if before_value == after_value {
                RevertFieldAction::Keep
            } else if current_value == after_value {
                RevertFieldAction::Restore {
                    value: before_value.cloned(),
                }
            } else {
                return Err(Conflict::FieldChanged { field_id });
            };
            Ok((field_id, action))
        })
        .collect::<Result<BTreeMap<_, _>, Conflict>>()?;

    Ok(RevertPlan {
        reverts_change_id: reverts_change_id.into(),
        fields,
    })
}

/// Compute a selective inverse while enforcing the current Form schema.
/// Deleted fields and incompatible values are explicit conflicts; no schema
/// resurrection or implicit coercion is attempted.
pub fn selective_inverse_with_schema(
    reverts_change_id: impl Into<String>,
    before: &BTreeMap<FieldId, FieldValue>,
    after: &BTreeMap<FieldId, FieldValue>,
    current: &BTreeMap<FieldId, FieldValue>,
    current_schema: &BTreeMap<FieldId, FieldType>,
) -> Result<RevertPlan, Conflict> {
    let plan = selective_inverse(reverts_change_id, before, after, current)?;
    for (field_id, action) in &plan.fields {
        let Some(field_type) = current_schema.get(field_id) else {
            if matches!(action, RevertFieldAction::Restore { value: Some(_) }) {
                return Err(Conflict::FieldDeleted {
                    field_id: *field_id,
                });
            }
            continue;
        };
        for value in [
            after.get(field_id),
            before.get(field_id),
            current.get(field_id),
        ]
        .into_iter()
        .flatten()
        {
            if !field_value_matches_type(value, field_type) {
                return Err(Conflict::IncompatibleType {
                    field_id: *field_id,
                });
            }
        }
    }
    Ok(plan)
}

/// Compute a selective inverse against the complete current Form schema.
/// This variant validates typed list items and AssetReferences; it is the
/// entry point storage adapters should use when they have Form definitions.
pub fn selective_inverse_with_form_schema(
    reverts_change_id: impl Into<String>,
    before: &BTreeMap<FieldId, FieldValue>,
    after: &BTreeMap<FieldId, FieldValue>,
    current: &BTreeMap<FieldId, FieldValue>,
    current_schema: &BTreeMap<FieldId, FormField>,
) -> Result<RevertPlan, Conflict> {
    let plan = selective_inverse(reverts_change_id, before, after, current)?;
    for (field_id, action) in &plan.fields {
        let Some(field) = current_schema.get(field_id) else {
            if matches!(action, RevertFieldAction::Restore { value: Some(_) }) {
                return Err(Conflict::FieldDeleted {
                    field_id: *field_id,
                });
            }
            continue;
        };
        for value in [
            after.get(field_id),
            before.get(field_id),
            current.get(field_id),
        ]
        .into_iter()
        .flatten()
        {
            if !field_value_matches_form_field(value, field) {
                return Err(Conflict::IncompatibleType {
                    field_id: *field_id,
                });
            }
        }
    }
    Ok(plan)
}

fn field_value_matches_type(value: &FieldValue, field_type: &FieldType) -> bool {
    if matches!(value, FieldValue::Null) {
        return true;
    }
    match (value, field_type) {
        (
            FieldValue::String(_),
            FieldType::String
            | FieldType::Markdown
            | FieldType::Sql
            | FieldType::RowReference
            | FieldType::Date
            | FieldType::Time
            | FieldType::Timestamp
            | FieldType::TimestampTz
            | FieldType::TimestampNs
            | FieldType::TimestampTzNs
            | FieldType::Uuid
            | FieldType::Binary,
        )
        | (FieldValue::Boolean(_), FieldType::Boolean)
        | (
            FieldValue::Integer(_),
            FieldType::Integer | FieldType::Long | FieldType::Float | FieldType::Double,
        )
        | (FieldValue::Number(_), FieldType::Float | FieldType::Double)
        | (FieldValue::Object(_), FieldType::ObjectList)
        | (FieldValue::AssetReference(_), FieldType::AssetReference) => true,
        (FieldValue::List(values), FieldType::List) => values
            .iter()
            .all(|value| matches!(value, FieldValue::String(_))),
        (FieldValue::List(values), FieldType::ObjectList) => values
            .iter()
            .all(|value| matches!(value, FieldValue::Object(_))),
        _ => false,
    }
}

fn field_value_matches_form_field(value: &FieldValue, field: &FormField) -> bool {
    if matches!(value, FieldValue::Null) {
        return true;
    }
    match (&field.field_type, value) {
        (FieldType::AssetReference, FieldValue::AssetReference(reference)) => {
            reference.validate().is_ok()
        }
        (FieldType::List, FieldValue::List(values)) => field
            .list_item
            .as_ref()
            .map(|item| {
                values
                    .iter()
                    .all(|value| field_value_matches_list_item(value, item))
            })
            .unwrap_or_else(|| {
                values
                    .iter()
                    .all(|value| matches!(value, FieldValue::String(_)))
            }),
        (FieldType::ObjectList, FieldValue::List(values)) => values
            .iter()
            .all(|value| matches!(value, FieldValue::Object(_))),
        (field_type, value) => field_value_matches_type(value, field_type),
    }
}

fn field_value_matches_list_item(value: &FieldValue, item: &ListItemDefinition) -> bool {
    if matches!(value, FieldValue::Null) {
        return true;
    }
    matches!(
        (&item.field_type, value),
        (FieldType::RowReference, FieldValue::String(_))
            | (FieldType::AssetReference, FieldValue::AssetReference(_))
            | (FieldType::String, FieldValue::String(_))
            | (FieldType::Markdown, FieldValue::String(_))
            | (FieldType::Sql, FieldValue::String(_))
            | (FieldType::Date, FieldValue::String(_))
            | (FieldType::Time, FieldValue::String(_))
            | (FieldType::Timestamp, FieldValue::String(_))
            | (FieldType::TimestampTz, FieldValue::String(_))
            | (FieldType::TimestampNs, FieldValue::String(_))
            | (FieldType::TimestampTzNs, FieldValue::String(_))
            | (FieldType::Uuid, FieldValue::String(_))
            | (FieldType::Binary, FieldValue::String(_))
            | (FieldType::Boolean, FieldValue::Boolean(_))
            | (FieldType::Integer, FieldValue::Integer(_))
            | (FieldType::Long, FieldValue::Integer(_))
            | (FieldType::Float, FieldValue::Integer(_))
            | (FieldType::Double, FieldValue::Integer(_))
            | (FieldType::Float, FieldValue::Number(_))
            | (FieldType::Double, FieldValue::Number(_))
    )
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChangeValidationError {
    field: &'static str,
    reason: &'static str,
    kind: ChangeValidationErrorKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ChangeValidationErrorKind {
    InvalidText,
    ActorMismatch,
}

impl fmt::Display for ChangeValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ChangeValidationErrorKind::InvalidText => {
                write!(formatter, "{} {}", self.field, self.reason)
            }
            ChangeValidationErrorKind::ActorMismatch => {
                formatter.write_str("authenticated actor does not match Change actor")
            }
        }
    }
}

impl std::error::Error for ChangeValidationError {}

fn validate_text(
    value: &str,
    field: &'static str,
    max_bytes: usize,
) -> Result<(), ChangeValidationError> {
    if value.trim().is_empty() {
        return Err(ChangeValidationError {
            field,
            reason: "must not be empty",
            kind: ChangeValidationErrorKind::InvalidText,
        });
    }
    if value.len() > max_bytes {
        return Err(ChangeValidationError {
            field,
            reason: "exceeds its maximum length",
            kind: ChangeValidationErrorKind::InvalidText,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::FieldId;

    fn field(value: i64) -> FieldValue {
        FieldValue::Integer(value)
    }

    #[test]
    fn selective_inverse_keeps_unrelated_current_fields() {
        let title = FieldId::new(100).unwrap();
        let body = FieldId::new(101).unwrap();
        let before = BTreeMap::from([(title, field(1)), (body, field(2))]);
        let after = BTreeMap::from([(title, field(3)), (body, field(2))]);
        let current = BTreeMap::from([(title, field(3)), (body, field(4))]);

        let plan = selective_inverse("change-1", &before, &after, &current).unwrap();
        assert_eq!(
            plan.fields.get(&title),
            Some(&RevertFieldAction::Restore {
                value: Some(field(1))
            })
        );
        assert_eq!(plan.fields.get(&body), Some(&RevertFieldAction::Keep));
    }

    #[test]
    fn selective_inverse_conflicts_on_a_later_same_field_change() {
        let field_id = FieldId::new(100).unwrap();
        let before = BTreeMap::from([(field_id, field(1))]);
        let after = BTreeMap::from([(field_id, field(2))]);
        let current = BTreeMap::from([(field_id, field(3))]);

        assert_eq!(
            selective_inverse("change-1", &before, &after, &current),
            Err(Conflict::FieldChanged { field_id })
        );
    }

    #[test]
    fn descriptor_rejects_missing_actor() {
        let command = ChangeCommand {
            change_id: "change-1".into(),
            run_id: None,
            actor_principal_id: " ".into(),
            message: None,
            reverts_change_id: None,
            created_at_micros: 1,
        };
        assert!(command.validate().is_err());
    }

    #[test]
    fn context_rejects_client_actor_impersonation() {
        let command = ChangeCommand {
            change_id: "change-1".into(),
            run_id: None,
            actor_principal_id: "principal:claimed".into(),
            message: None,
            reverts_change_id: None,
            created_at_micros: 1,
        };
        assert!(command
            .into_context("principal:authenticated", None)
            .is_err());
    }

    #[test]
    fn schema_guard_rejects_deleted_restore_field() {
        let field_id = FieldId::new(100).unwrap();
        let before = BTreeMap::from([(field_id, field(1))]);
        let after = BTreeMap::from([(field_id, field(2))]);
        let current = after.clone();
        assert_eq!(
            selective_inverse_with_schema("change-1", &before, &after, &current, &BTreeMap::new(),),
            Err(Conflict::FieldDeleted { field_id })
        );
    }

    #[test]
    fn form_schema_guard_rejects_wrong_typed_list_item() {
        let field_id = FieldId::new(100).unwrap();
        let field = FormField {
            id: field_id,
            name: "numbers".into(),
            field_type: FieldType::List,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: Some(ListItemDefinition {
                field_type: FieldType::Integer,
                reference_form: None,
            }),
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        };
        let before = BTreeMap::from([(
            field_id,
            FieldValue::List(vec![FieldValue::String("wrong".into())]),
        )]);
        let after = BTreeMap::from([(field_id, FieldValue::List(vec![FieldValue::Integer(2)]))]);
        assert_eq!(
            selective_inverse_with_form_schema(
                "change-1",
                &before,
                &after,
                &after,
                &BTreeMap::from([(field_id, field)]),
            ),
            Err(Conflict::IncompatibleType { field_id })
        );
    }
}
