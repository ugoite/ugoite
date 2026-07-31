use crate::id::{FieldId, FormId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FormVersion(u32);

impl FormVersion {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if value == 0 {
            return Err(DomainError::InvalidFormVersion);
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> u32 {
        self.0
    }
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Markdown,
    Sql,
    Boolean,
    Integer,
    Long,
    Float,
    Double,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    TimestampNs,
    TimestampTzNs,
    Uuid,
    Binary,
    List,
    ObjectList,
    RowReference,
}

impl FieldType {
    pub fn can_widen_to(&self, target: &Self) -> bool {
        self == target
            || matches!(
                (self, target),
                (Self::Integer, Self::Long)
                    | (Self::Integer | Self::Long | Self::Float, Self::Double)
            )
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub id: FieldId,
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_form: Option<FormId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct FormDefinition {
    pub id: FormId,
    pub version: FormVersion,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: Vec<FormField>,
    #[serde(default)]
    pub allow_extra_attributes: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extension_metadata: BTreeMap<String, Value>,
}

/// Returns the stable ASCII SQL relation exposed for a Form.
///
/// SQL identity follows the immutable Form identity rather than its editable
/// display name. This keeps the relation unique for every valid Form set and
/// preserves Saved SQL across Form renames.
pub fn sql_relation_name(form_id: FormId) -> String {
    format!("form_{}", form_id.as_uuid().simple())
}

/// Returns the stable ASCII SQL column exposed for a Form field.
///
/// Field names are editable labels. Field IDs are the immutable identity that
/// Iceberg already uses for schema evolution, so SQL follows that identity.
pub fn sql_column_name(field_id: FieldId) -> String {
    format!("field_{}", field_id.get())
}

impl FormDefinition {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.name.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for field in &self.fields {
            if field.name.trim().is_empty() {
                return Err(DomainError::EmptyName);
            }
            if !ids.insert(field.id) {
                return Err(DomainError::DuplicateFieldId(field.id));
            }
            if field.id.get() < 100 {
                return Err(DomainError::ReservedFieldId(field.id));
            }
            if !names.insert(field.name.as_str()) {
                return Err(DomainError::DuplicateFieldName(field.name.clone()));
            }
        }
        Ok(())
    }

    pub fn apply(&self, changes: &FormChangeSet) -> Result<Self, DomainError> {
        if changes.form_id != self.id {
            return Err(DomainError::FormIdChanged);
        }
        if let Some(expected_version) = changes.expected_version {
            if expected_version != self.version {
                return Err(DomainError::VersionConflict);
            }
        }
        let mut next = self.clone();
        for change in &changes.changes {
            match change {
                FormChange::AddField(field) => {
                    if next.fields.iter().any(|existing| existing.id == field.id) {
                        return Err(DomainError::DuplicateFieldId(field.id));
                    }
                    next.fields.push(field.clone());
                }
                FormChange::RenameField { field_id, name } => {
                    next.field_mut(*field_id)?.name = name.clone()
                }
                FormChange::ChangeFieldType {
                    field_id,
                    field_type,
                } => next.field_mut(*field_id)?.field_type = field_type.clone(),
                FormChange::ChangeRequired { field_id, required } => {
                    next.field_mut(*field_id)?.required = *required
                }
                FormChange::UpdateFieldMetadata {
                    field_id,
                    label,
                    description,
                    semantic_role,
                    validation,
                    enum_values,
                } => {
                    let field = next.field_mut(*field_id)?;
                    field.label.clone_from(label);
                    field.description.clone_from(description);
                    field.semantic_role.clone_from(semantic_role);
                    field.validation.clone_from(validation);
                    field.enum_values.clone_from(enum_values);
                }
                FormChange::DeprecateField { field_id } => {
                    next.field_mut(*field_id)?.deprecated = true
                }
                FormChange::RestoreField { field_id } => {
                    next.field_mut(*field_id)?.deprecated = false
                }
                FormChange::RenameForm { name } => next.name = name.clone(),
                FormChange::UpdateFormMetadata {
                    description,
                    allow_extra_attributes,
                    extension_metadata,
                } => {
                    next.description.clone_from(description);
                    next.allow_extra_attributes = *allow_extra_attributes;
                    next.extension_metadata.clone_from(extension_metadata);
                }
            }
        }
        next.version = self.version.next();
        next.validate()?;
        Ok(next)
    }

    fn field_mut(&mut self, id: FieldId) -> Result<&mut FormField, DomainError> {
        self.fields
            .iter_mut()
            .find(|field| field.id == id)
            .ok_or(DomainError::UnknownField(id))
    }
}

#[cfg(test)]
mod tests {
    use super::{sql_column_name, sql_relation_name};
    use crate::id::{FieldId, FormId};
    use uuid::Uuid;

    #[test]
    fn sql_names_follow_immutable_ids() {
        let form_id = FormId::from(Uuid::from_u128(1));
        assert_eq!(
            sql_relation_name(form_id),
            "form_00000000000000000000000000000001"
        );
        assert_eq!(sql_column_name(FieldId::new(104).unwrap()), "field_104");
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct FormChangeSet {
    pub form_id: FormId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<FormVersion>,
    pub changes: Vec<FormChange>,
}

impl FormChangeSet {
    pub fn compatibility(&self, current: &FormDefinition) -> Result<Compatibility, DomainError> {
        if self.form_id != current.id {
            return Err(DomainError::FormIdChanged);
        }
        let mut result = Compatibility::Compatible;
        for change in &self.changes {
            let compatibility = match change {
                FormChange::AddField(_)
                | FormChange::RenameField { .. }
                | FormChange::RenameForm { .. }
                | FormChange::UpdateFieldMetadata { .. }
                | FormChange::UpdateFormMetadata { .. }
                | FormChange::DeprecateField { .. }
                | FormChange::RestoreField { .. } => Compatibility::Compatible,
                FormChange::ChangeRequired { .. } => Compatibility::Compatible,
                FormChange::ChangeFieldType {
                    field_id,
                    field_type,
                } => {
                    let source = current
                        .fields
                        .iter()
                        .find(|field| field.id == *field_id)
                        .ok_or(DomainError::UnknownField(*field_id))?;
                    if source.field_type.can_widen_to(field_type) {
                        Compatibility::Compatible
                    } else {
                        Compatibility::Breaking
                    }
                }
            };
            result = result.max(compatibility);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum FormChange {
    AddField(FormField),
    RenameField {
        field_id: FieldId,
        name: String,
    },
    ChangeFieldType {
        field_id: FieldId,
        field_type: FieldType,
    },
    ChangeRequired {
        field_id: FieldId,
        required: bool,
    },
    UpdateFieldMetadata {
        field_id: FieldId,
        label: Option<String>,
        description: Option<String>,
        semantic_role: Option<String>,
        validation: Option<Value>,
        enum_values: Vec<String>,
    },
    DeprecateField {
        field_id: FieldId,
    },
    RestoreField {
        field_id: FieldId,
    },
    RenameForm {
        name: String,
    },
    UpdateFormMetadata {
        description: Option<String>,
        allow_extra_attributes: bool,
        extension_metadata: BTreeMap<String, Value>,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    Compatible,
    MigrationRequired,
    Breaking,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DomainError {
    InvalidFormVersion,
    EmptyName,
    FormIdChanged,
    DuplicateFieldId(FieldId),
    DuplicateFieldName(String),
    UnknownField(FieldId),
    ReservedFieldId(FieldId),
    VersionConflict,
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for DomainError {}
