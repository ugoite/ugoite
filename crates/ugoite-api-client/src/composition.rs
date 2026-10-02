//! Portable Composition read response DTOs.
//!
//! These types describe the wire contract without depending on the storage
//! representation used by the Server.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionRawRevision {
    pub revision: CompositionRevisionMetadata,
    pub fields: BTreeMap<String, Value>,
    pub unmapped_field_values: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionRevisionMetadata {
    pub form_id: String,
    pub entry_id: String,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    pub entry_version: u64,
    pub change_id: String,
    pub expected_version: Option<u64>,
    /// `upsert`, `delete`, or `restore`.
    pub operation: String,
    pub committed_at_micros: i64,
    pub author_id: String,
    pub form_version: u32,
    pub source_kind: String,
    pub source_id: Option<String>,
    pub entry: CompositionEntryMetadata,
    pub extra_attributes: BTreeMap<String, Value>,
    pub extension_metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionEntryMetadata {
    pub external_id: String,
    pub tags: Vec<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
    pub updated_by: String,
    pub integrity: CompositionEntryIntegrity,
    pub deleted: bool,
    pub deleted_at_micros: Option<i64>,
    pub deleted_by: Option<String>,
    pub restored_from: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CompositionEntryIntegrity {
    pub checksum: String,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionHistoryPage {
    pub entry_id: String,
    pub revisions: Vec<CompositionRawRevision>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}
