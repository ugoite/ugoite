//! Pure identifiers and contracts for rebuildable derived relations.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use uuid::Uuid;

/// Stable identity for a derived relation.  It is deliberately independent of
/// the display name and of the current producer implementation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DerivedRelationId(Uuid);

impl DerivedRelationId {
    pub const ASSET_TEXT: Self = Self(Uuid::from_u128(0x0000000000000000000000000000a001));

    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for DerivedRelationId {
    fn from(value: Uuid) -> Self {
        Self::from_uuid(value)
    }
}

impl fmt::Display for DerivedRelationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A field type shared by typed Form and derived relation schemas without
/// importing any Iceberg, Arrow, or runtime dependencies into the domain.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedValueType {
    String,
    Long,
    Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelationField {
    pub field_id: i32,
    pub name: String,
    pub value_type: DerivedValueType,
    #[serde(default)]
    pub nullable: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypedSchema {
    pub fields: Vec<RelationField>,
}

impl TypedSchema {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for field in &self.fields {
            if field.field_id <= 0 || !ids.insert(field.field_id) {
                return Err(format!(
                    "invalid or duplicate derived field ID {}",
                    field.field_id
                ));
            }
            if field.name.trim().is_empty() || !names.insert(field.name.as_str()) {
                return Err(format!(
                    "invalid or duplicate derived field name {:?}",
                    field.name
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedExposure {
    Internal,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DerivedRelationDefinition {
    pub relation_id: DerivedRelationId,
    pub name: String,
    pub definition_version: u32,
    pub schema: TypedSchema,
    pub logical_key: Vec<String>,
    pub exposure: DerivedExposure,
    pub producer_id: String,
}

impl DerivedRelationDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.producer_id.trim().is_empty() {
            return Err("derived relation name and producer are required".into());
        }
        if self.definition_version == 0 {
            return Err("derived relation definition version must be positive".into());
        }
        self.schema.validate()?;
        if self.logical_key.is_empty() {
            return Err("derived relation logical key must not be empty".into());
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> String {
        sha256_digest(&canonical_json(self).expect("definition serialization"))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerIdentity {
    pub producer_id: String,
    pub producer_fingerprint: String,
    pub compatibility_epoch: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedStatus {
    Ready,
    Empty,
    Unsupported,
    Failed,
    Missing,
    SourceMismatch,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedErrorCode {
    AssetMissing,
    AssetParserFailed,
    AssetParserLimit,
    AssetUnsupportedFormat,
    AssetSourceChanged,
    AssetSizeMismatch,
    AssetChecksumMismatch,
    SourceRevisionIntegrityFailed,
    SourceCoordinateOverflow,
    SourceReferenceOverflow,
    DerivedMaterializationInvalid,
    DerivedUnavailable,
}

impl DerivedErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AssetMissing => "asset_missing",
            Self::AssetParserFailed => "asset_parser_failed",
            Self::AssetParserLimit => "asset_parser_limit",
            Self::AssetUnsupportedFormat => "asset_unsupported_format",
            Self::AssetSourceChanged => "asset_source_changed",
            Self::AssetSizeMismatch => "asset_size_mismatch",
            Self::AssetChecksumMismatch => "asset_checksum_mismatch",
            Self::SourceRevisionIntegrityFailed => "source_revision_integrity_failed",
            Self::SourceCoordinateOverflow => "source_coordinate_overflow",
            Self::SourceReferenceOverflow => "source_reference_overflow",
            Self::DerivedMaterializationInvalid => "derived_materialization_invalid",
            Self::DerivedUnavailable => "derived_unavailable",
        }
    }
}

pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    fn sort(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sort).collect())
            }
            serde_json::Value::Object(values) => serde_json::Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, sort(value)))
                    .collect::<serde_json::Map<_, _>>(),
            ),
            scalar => scalar,
        }
    }

    serde_json::to_vec(&sort(serde_json::to_value(value)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_text_identity_is_stable() {
        assert_eq!(
            DerivedRelationId::ASSET_TEXT.to_string(),
            "00000000-0000-0000-0000-00000000a001"
        );
    }

    #[test]
    fn definition_fingerprint_is_deterministic() {
        let definition = DerivedRelationDefinition {
            relation_id: DerivedRelationId::ASSET_TEXT,
            name: "asset_text".into(),
            definition_version: 1,
            schema: TypedSchema {
                fields: vec![RelationField {
                    field_id: 1,
                    name: "asset_id".into(),
                    value_type: DerivedValueType::String,
                    nullable: false,
                }],
            },
            logical_key: vec!["asset_id".into()],
            exposure: DerivedExposure::Internal,
            producer_id: "ugoite.asset_text".into(),
        };
        assert_eq!(definition.fingerprint(), definition.fingerprint());
    }
}
