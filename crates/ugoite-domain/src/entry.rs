use crate::form::{FieldType, FormDefinition, FormVersion};
use crate::id::{EntryId, FieldId, FormId, RevisionId};
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
    String(String),
    Boolean(bool),
    Integer(i64),
    Number(#[serde(with = "finite_f64")] f64),
    List(Vec<FieldValue>),
    Object(BTreeMap<String, FieldValue>),
    Null,
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
                if matches!(self.values.get(&field.id), None | Some(FieldValue::Null)) {
                    return Err(RevisionError::RequiredField(field.id));
                }
            }
            for (field_id, value) in &self.values {
                let field = form
                    .fields
                    .iter()
                    .find(|field| field.id == *field_id)
                    .ok_or(RevisionError::UnknownField(*field_id))?;
                if !value_matches_type(value, &field.field_type) {
                    return Err(RevisionError::WrongType(*field_id));
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

fn value_matches_type(value: &FieldValue, field_type: &FieldType) -> bool {
    if matches!(value, FieldValue::Null) {
        return true;
    }
    match (value, field_type) {
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
            | FieldType::Binary
            | FieldType::RowReference,
        )
        | (FieldValue::Boolean(_), FieldType::Boolean)
        | (
            FieldValue::Integer(_),
            FieldType::Integer | FieldType::Long | FieldType::Float | FieldType::Double,
        )
        | (FieldValue::Number(_), FieldType::Float | FieldType::Double) => true,
        (FieldValue::List(values), FieldType::List) => values
            .iter()
            .all(|value| matches!(value, FieldValue::String(_))),
        (FieldValue::List(values), FieldType::ObjectList) => values
            .iter()
            .all(|value| matches!(value, FieldValue::Object(_))),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RevisionError {
    WrongForm,
    WrongFormVersion,
    MissingProvenance,
    VersionConflict,
    TombstoneHasValues,
    ExtraAttributesNotAllowed,
    RequiredField(FieldId),
    UnknownField(FieldId),
    WrongType(FieldId),
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
