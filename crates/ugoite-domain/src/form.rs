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
    AssetReference,
}

impl FieldType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Markdown => "markdown",
            Self::Sql => "sql",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
            Self::Date => "date",
            Self::Time => "time",
            Self::Timestamp => "timestamp",
            Self::TimestampTz => "timestamp_tz",
            Self::TimestampNs => "timestamp_ns",
            Self::TimestampTzNs => "timestamp_tz_ns",
            Self::Uuid => "uuid",
            Self::Binary => "binary",
            Self::List => "list",
            Self::ObjectList => "object_list",
            Self::RowReference => "row_reference",
            Self::AssetReference => "asset_reference",
        }
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
    /// Item schema for a typed `list` field. Keeping this separate from
    /// `FieldType::List` preserves the existing JSON shape while making the
    /// item type part of the persisted Form definition.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "items")]
    pub list_item: Option<ListItemDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListItemDefinition {
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "target_form"
    )]
    pub reference_form: Option<FormId>,
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
            match field.field_type {
                FieldType::RowReference if field.reference_form.is_none() => {
                    return Err(DomainError::ReferenceTargetMissing(field.id));
                }
                FieldType::RowReference => {}
                FieldType::List if field.list_item.is_none() => {}
                FieldType::List => {
                    let item = field.list_item.as_ref().expect("checked above");
                    if matches!(item.field_type, FieldType::List | FieldType::ObjectList) {
                        return Err(DomainError::InvalidListItemType(field.id));
                    }
                    if matches!(item.field_type, FieldType::RowReference)
                        && item.reference_form.is_none()
                    {
                        return Err(DomainError::ReferenceTargetMissing(field.id));
                    }
                    if !matches!(item.field_type, FieldType::RowReference)
                        && item.reference_form.is_some()
                    {
                        return Err(DomainError::InvalidReferenceTarget(field.id));
                    }
                }
                _ if field.list_item.is_some() => {
                    return Err(DomainError::InvalidListItemType(field.id));
                }
                _ if field.reference_form.is_some() => {
                    return Err(DomainError::InvalidReferenceTarget(field.id));
                }
                _ => {}
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
                    reference_form,
                    list_item,
                    validation,
                    enum_values,
                } => {
                    let field = next.field_mut(*field_id)?;
                    field.label.clone_from(label);
                    field.description.clone_from(description);
                    field.semantic_role.clone_from(semantic_role);
                    field.reference_form.clone_from(reference_form);
                    field.list_item.clone_from(list_item);
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
                FormChange::ChangeFieldType { field_id, .. } => {
                    current
                        .fields
                        .iter()
                        .find(|field| field.id == *field_id)
                        .ok_or(DomainError::UnknownField(*field_id))?;
                    Compatibility::Breaking
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
        #[serde(default)]
        reference_form: Option<FormId>,
        #[serde(default)]
        list_item: Option<ListItemDefinition>,
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
    ReferenceTargetMissing(FieldId),
    InvalidReferenceTarget(FieldId),
    InvalidListItemType(FieldId),
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for DomainError {}
