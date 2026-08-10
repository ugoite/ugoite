use crate::form::{FieldType, FormDefinition, FormVersion};
use crate::id::{validate_asset_id, EntryId, FieldId, FormId, RevisionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryOperation {
    Upsert,
    Delete,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    AssetReference(AssetReference),
    String(String),
    Boolean(bool),
    Integer(i64),
    Number(#[serde(with = "finite_f64")] f64),
    List(Vec<FieldValue>),
    Object(BTreeMap<String, FieldValue>),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetReference {
    pub asset_id: String,
    pub name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AssetReferenceError {
    InvalidAssetId,
    EmptyName,
    EmptyMediaType,
    InvalidChecksum,
}

impl fmt::Display for AssetReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidAssetId => "asset_id is invalid",
            Self::EmptyName => "name must not be empty",
            Self::EmptyMediaType => "media_type must not be empty",
            Self::InvalidChecksum => "sha256 must be 64 lowercase hexadecimal characters",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AssetReferenceError {}

impl AssetReference {
    /// Validate the logical value stored in a Form field.
    ///
    /// This is deliberately independent from object storage. An AssetReference
    /// is valid only when its metadata is complete and its ID follows the same
    /// identifier contract used by Asset byte APIs.
    pub fn validate(&self) -> Result<(), AssetReferenceError> {
        validate_asset_id(&self.asset_id).map_err(|_| AssetReferenceError::InvalidAssetId)?;
        if self.name.trim().is_empty() {
            return Err(AssetReferenceError::EmptyName);
        }
        if self.media_type.trim().is_empty() {
            return Err(AssetReferenceError::EmptyMediaType);
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(AssetReferenceError::InvalidChecksum);
        }
        Ok(())
    }
}

/// Fixed metadata that accompanies every revision independently of a Form's
/// typed columns. It is part of the canonical revision model, not an opaque
/// JSON payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMetadata {
    /// Stable API-facing identity stored beside the UUID revision key.
    #[serde(default)]
    pub external_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at_micros: i64,
    #[serde(default)]
    pub updated_at_micros: i64,
    /// Principal that authored the latest revision, including delete and
    /// restore revisions.
    #[serde(default)]
    pub updated_by: String,
    #[serde(default)]
    pub integrity: EntryIntegrity,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_micros: Option<i64>,
    /// Principal that authored the current delete tombstone. It is cleared by
    /// a restore and remains available on historical delete revisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<RevisionId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryIntegrity {
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryRevision {
    pub form_id: FormId,
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
    pub parent_revision_id: Option<RevisionId>,
    pub entry_version: u64,
    pub expected_version: Option<u64>,
    pub operation: EntryOperation,
    pub committed_at_micros: i64,
    pub author_id: String,
    pub form_version: FormVersion,
    pub source_kind: String,
    pub source_id: Option<String>,
    #[serde(default)]
    pub entry: EntryMetadata,
    pub values: BTreeMap<FieldId, FieldValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra_attributes: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extension_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryRevisionDraft {
    pub form_id: FormId,
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
    pub operation: EntryOperation,
    pub committed_at_micros: i64,
    pub author_id: String,
    pub form_version: FormVersion,
    pub source_kind: String,
    pub source_id: Option<String>,
    #[serde(default)]
    pub entry: EntryMetadata,
    pub values: BTreeMap<FieldId, FieldValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra_attributes: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extension_metadata: BTreeMap<String, Value>,
}

impl EntryRevisionDraft {
    pub fn build(
        self,
        form: &FormDefinition,
        current: Option<&EntryRevision>,
    ) -> Result<EntryRevision, RevisionError> {
        let (parent_revision_id, expected_version, entry_version) =
            current.map_or((None, None, 1), |revision| {
                (
                    Some(revision.revision_id),
                    Some(revision.entry_version),
                    revision.entry_version.saturating_add(1),
                )
            });
        let revision = EntryRevision {
            form_id: self.form_id,
            entry_id: self.entry_id,
            revision_id: self.revision_id,
            parent_revision_id,
            entry_version,
            expected_version,
            operation: self.operation,
            committed_at_micros: self.committed_at_micros,
            author_id: self.author_id,
            form_version: self.form_version,
            source_kind: self.source_kind,
            source_id: self.source_id,
            entry: self.entry,
            values: self.values,
            extra_attributes: self.extra_attributes,
            extension_metadata: self.extension_metadata,
        };
        revision.validate(form, current)?;
        Ok(revision)
    }
}

impl EntryRevision {
    pub fn validate_payload(&self, form: &FormDefinition) -> Result<(), RevisionError> {
        if self.form_id != form.id {
            return Err(RevisionError::WrongForm);
        }
        if self.form_version != form.version {
            return Err(RevisionError::WrongFormVersion);
        }
        if self.entry_version == 0
            || self.author_id.trim().is_empty()
            || self.source_kind.trim().is_empty()
        {
            return Err(RevisionError::MissingProvenance);
        }
        if self.entry.updated_by.trim().is_empty() {
            return Err(RevisionError::InvalidAttribution);
        }
        if self.entry_version == 1
            && self.expected_version.is_none()
            && self.parent_revision_id.is_none()
            && self.entry.updated_by != self.author_id
        {
            return Err(RevisionError::InvalidAttribution);
        }
        match self.operation {
            EntryOperation::Delete
                if !self.entry.deleted
                    || self.entry.deleted_at_micros.is_none()
                    || self.entry.deleted_by.as_deref() != Some(self.entry.updated_by.as_str()) =>
            {
                return Err(RevisionError::InvalidAttribution)
            }
            EntryOperation::Upsert | EntryOperation::Restore
                if self.entry.deleted
                    || self.entry.deleted_at_micros.is_some()
                    || self.entry.deleted_by.is_some() =>
            {
                return Err(RevisionError::InvalidAttribution)
            }
            _ => {}
        }
        if self.operation == EntryOperation::Delete && !self.values.is_empty() {
            return Err(RevisionError::TombstoneHasValues);
        }
        if !form.allow_extra_attributes && !self.extra_attributes.is_empty() {
            return Err(RevisionError::ExtraAttributesNotAllowed);
        }
        if self.operation != EntryOperation::Delete {
            for field in form
                .fields
                .iter()
                .filter(|field| field.required && !field.deprecated)
            {
                let missing = match self.values.get(&field.id) {
                    None | Some(FieldValue::Null) => true,
                    Some(FieldValue::List(values)) => values.is_empty(),
                    Some(_) => false,
                };
                if missing {
                    return Err(RevisionError::RequiredField(field.id));
                }
            }
            for (field_id, value) in &self.values {
                let field = form
                    .fields
                    .iter()
                    .find(|field| field.id == *field_id)
                    .ok_or(RevisionError::UnknownField(*field_id))?;
                if !value_matches_type(value, field) {
                    return Err(RevisionError::WrongType(*field_id));
                }
                if field.field_type == FieldType::AssetReference {
                    if let FieldValue::AssetReference(reference) = value {
                        reference
                            .validate()
                            .map_err(|_| RevisionError::InvalidAssetReference(*field_id))?;
                    }
                }
                if field.field_type == FieldType::List
                    && field
                        .list_item
                        .as_ref()
                        .is_some_and(|item| item.field_type == FieldType::AssetReference)
                {
                    if let FieldValue::List(values) = value {
                        let mut asset_ids = std::collections::BTreeSet::new();
                        for value in values {
                            if let FieldValue::AssetReference(reference) = value {
                                reference
                                    .validate()
                                    .map_err(|_| RevisionError::InvalidAssetReference(*field_id))?;
                                if !asset_ids.insert(reference.asset_id.as_str()) {
                                    return Err(RevisionError::DuplicateAssetReference(*field_id));
                                }
                            } else {
                                return Err(RevisionError::WrongType(*field_id));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate(
        &self,
        form: &FormDefinition,
        current: Option<&Self>,
    ) -> Result<(), RevisionError> {
        self.validate_payload(form)?;
        match current {
            None if self.expected_version.is_some()
                || self.parent_revision_id.is_some()
                || self.entry_version != 1 =>
            {
                return Err(RevisionError::VersionConflict)
            }
            Some(previous) if self.author_id != previous.author_id => {
                return Err(RevisionError::AuthorChanged)
            }
            Some(previous)
                if self.expected_version != Some(previous.entry_version)
                    || self.parent_revision_id != Some(previous.revision_id)
                    || self.entry_version
                        != previous
                            .entry_version
                            .checked_add(1)
                            .ok_or(RevisionError::VersionConflict)? =>
            {
                return Err(RevisionError::VersionConflict)
            }
            _ => {}
        }
        Ok(())
    }
}

fn value_matches_type(value: &FieldValue, field: &crate::form::FormField) -> bool {
    if matches!(value, FieldValue::Null) {
        return true;
    }
    match (value, &field.field_type) {
        (FieldValue::String(_), FieldType::RowReference)
        | (FieldValue::AssetReference(_), FieldType::AssetReference) => true,
        (
            FieldValue::String(_),
            FieldType::String
            | FieldType::Markdown
            | FieldType::Sql
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
        | (FieldValue::Number(_), FieldType::Float | FieldType::Double) => true,
        (FieldValue::List(values), FieldType::List) => field
            .list_item
            .as_ref()
            .map(|item| {
                values
                    .iter()
                    .all(|value| value_matches_list_item(value, item))
            })
            .unwrap_or_else(|| {
                values
                    .iter()
                    .all(|value| matches!(value, FieldValue::String(_)))
            }),
        (FieldValue::List(values), FieldType::ObjectList) => values
            .iter()
            .all(|value| matches!(value, FieldValue::Object(_))),
        _ => false,
    }
}

fn value_matches_list_item(value: &FieldValue, item: &crate::form::ListItemDefinition) -> bool {
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
            | (_, FieldValue::Null)
    )
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RevisionError {
    WrongForm,
    WrongFormVersion,
    MissingProvenance,
    AuthorChanged,
    InvalidAttribution,
    VersionConflict,
    TombstoneHasValues,
    ExtraAttributesNotAllowed,
    RequiredField(FieldId),
    UnknownField(FieldId),
    WrongType(FieldId),
    InvalidAssetReference(FieldId),
    DuplicateAssetReference(FieldId),
}
impl fmt::Display for RevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for RevisionError {}

mod finite_f64 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(*value)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let value = f64::deserialize(deserializer)?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(serde::de::Error::custom("number must be finite"))
        }
    }
}
