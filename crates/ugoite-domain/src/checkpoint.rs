//! Portable descriptions of reproducible Space read coordinates.
//!
//! A checkpoint contains storage coordinates only.  It deliberately carries no
//! authorization, SQL, or query-policy state.

use crate::id::{FormId, SpaceId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const SPACE_CHECKPOINT_FORMAT_VERSION: u32 = 1;

/// One immutable Iceberg table coordinate reachable from a Catalog Head.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckpointTable {
    pub form_id: FormId,
    /// The Form relation captured from immutable Iceberg metadata. It lets a
    /// query session resolve one requested Form without loading every Form
    /// definition from the checkpoint.
    pub form_name: String,
    pub namespace: Vec<String>,
    pub table: String,
    pub table_uuid: String,
    pub metadata_location: String,
    pub snapshot_id: Option<i64>,
    pub schema_id: i32,
}

/// A reproducible, Space-wide read coordinate.
///
/// `created_at_micros` and `name` describe a capture but are intentionally
/// excluded from `coordinate_checksum`: two captures of the same coordinates
/// have the same identity.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpaceCheckpoint {
    pub format_version: u32,
    pub space_id: SpaceId,
    pub catalog_generation: u64,
    pub catalog_head_checksum: String,
    pub publication_location: String,
    pub publication_checksum: String,
    pub form_registry_generation: u64,
    pub tables: Vec<CheckpointTable>,
    pub coordinate_checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_micros: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl SpaceCheckpoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        space_id: SpaceId,
        catalog_generation: u64,
        catalog_head_checksum: String,
        publication_location: String,
        publication_checksum: String,
        form_registry_generation: u64,
        tables: Vec<CheckpointTable>,
    ) -> Self {
        let mut checkpoint = Self {
            format_version: SPACE_CHECKPOINT_FORMAT_VERSION,
            space_id,
            catalog_generation,
            catalog_head_checksum,
            publication_location,
            publication_checksum,
            form_registry_generation,
            tables,
            coordinate_checksum: String::new(),
            created_at_micros: None,
            name: None,
        };
        checkpoint.coordinate_checksum = checkpoint.computed_coordinate_checksum();
        checkpoint
    }

    pub fn validate_coordinate_checksum(&self) -> bool {
        self.format_version == SPACE_CHECKPOINT_FORMAT_VERSION
            && self.coordinate_checksum == self.computed_coordinate_checksum()
    }

    /// Reject ambiguous or malformed coordinates before any storage is read.
    /// Integrity of the coordinates themselves is subsequently proven against
    /// their immutable publication and Catalog Head by the Iceberg adapter.
    pub fn validate_structure(&self) -> Result<(), &'static str> {
        let mut forms = BTreeSet::new();
        let mut form_names = BTreeSet::new();
        let mut identifiers = BTreeSet::new();
        for table in &self.tables {
            if !forms.insert(table.form_id) {
                return Err("checkpoint contains a duplicate Form ID");
            }
            if table.form_name.trim().is_empty() {
                return Err("checkpoint Form relation is empty");
            }
            if !form_names.insert(table.form_name.to_ascii_lowercase()) {
                return Err("checkpoint contains duplicate Form relations");
            }
            if table.namespace.is_empty() || table.namespace.iter().any(|part| part.is_empty()) {
                return Err("checkpoint table namespace is empty or invalid");
            }
            if table.table.is_empty() {
                return Err("checkpoint table name is empty");
            }
            if !identifiers.insert((table.namespace.clone(), table.table.clone())) {
                return Err("checkpoint assigns one table identifier to multiple Forms");
            }
            if Uuid::parse_str(&table.table_uuid).is_err() {
                return Err("checkpoint table UUID is invalid");
            }
            if table.metadata_location.is_empty() {
                return Err("checkpoint metadata location is empty");
            }
        }
        Ok(())
    }

    pub fn computed_coordinate_checksum(&self) -> String {
        let mut tables = self.tables.clone();
        tables.sort_by(|left, right| {
            (
                left.form_id,
                &left.namespace,
                &left.table,
                &left.metadata_location,
            )
                .cmp(&(
                    right.form_id,
                    &right.namespace,
                    &right.table,
                    &right.metadata_location,
                ))
        });
        let coordinate = CoordinateIdentity {
            format_version: self.format_version,
            space_id: self.space_id,
            catalog_generation: self.catalog_generation,
            catalog_head_checksum: &self.catalog_head_checksum,
            publication_location: &self.publication_location,
            publication_checksum: &self.publication_checksum,
            form_registry_generation: self.form_registry_generation,
            tables: &tables,
        };
        let bytes = serde_json::to_vec(&coordinate)
            .expect("checkpoint coordinate identity always serializes");
        hex::encode(Sha256::digest(bytes))
    }
}

#[derive(Serialize)]
struct CoordinateIdentity<'a> {
    format_version: u32,
    space_id: SpaceId,
    catalog_generation: u64,
    catalog_head_checksum: &'a str,
    publication_location: &'a str,
    publication_checksum: &'a str,
    form_registry_generation: u64,
    tables: &'a [CheckpointTable],
}

#[cfg(test)]
mod tests {
    use super::{CheckpointTable, SpaceCheckpoint};
    use crate::id::{FormId, SpaceId};
    use uuid::Uuid;

    fn checkpoint() -> SpaceCheckpoint {
        SpaceCheckpoint::new(
            SpaceId::from(Uuid::from_u128(1)),
            9,
            "head".into(),
            "publication".into(),
            "publication-checksum".into(),
            2,
            vec![CheckpointTable {
                form_id: FormId::from(Uuid::from_u128(2)),
                form_name: "form".into(),
                namespace: vec!["space_1".into()],
                table: "form_2".into(),
                table_uuid: "table".into(),
                metadata_location: "memory:///metadata.json".into(),
                snapshot_id: Some(4),
                schema_id: 3,
            }],
        )
    }

    #[test]
    fn metadata_does_not_change_coordinate_identity() {
        let first = checkpoint();
        let mut second = first.clone();
        second.created_at_micros = Some(42);
        second.name = Some("before-migration".into());

        assert_eq!(
            first.coordinate_checksum,
            second.computed_coordinate_checksum()
        );
        assert!(second.validate_coordinate_checksum());
    }
}
