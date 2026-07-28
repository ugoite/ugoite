//! Read-only Catalog Head and Iceberg metadata health evidence.

use serde::Serialize;

/// A read-only Space health report. Physical storage locations are deliberately
/// omitted so this value is safe to return from the normal REST API.
#[derive(Debug, Clone, Serialize)]
pub struct SpaceHealthReport {
    pub status: HealthStatus,
    pub catalog_head: CatalogHeadHealth,
    pub tables: Vec<TableHealth>,
    pub checkpoints: Vec<CheckpointHealth>,
    pub unreachable_failed_attempts: Vec<FailedAttemptCandidate>,
    pub unavailable_capabilities: Vec<&'static str>,
    pub recommendations: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogHeadHealth {
    pub readable: bool,
    pub checksum: Option<String>,
    pub etag: Option<String>,
    pub generation: Option<u64>,
    pub form_registry_generation: Option<u64>,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableHealth {
    pub status: HealthStatus,
    pub identifier: TableIdentifierHealth,
    pub form_id: Option<String>,
    pub table_uuid: String,
    pub metadata_location_redacted: bool,
    pub schema_id: Option<i32>,
    pub snapshot_id: Option<i64>,
    pub snapshot_count: Option<usize>,
    pub manifest_count: Option<usize>,
    pub manifest_size_bytes: Option<i64>,
    pub total_record_count: Option<u64>,
    pub total_data_file_count: Option<u64>,
    pub total_data_file_size_bytes: Option<u64>,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableIdentifierHealth {
    pub namespace: Vec<String>,
    pub table: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckpointHealth {
    pub name: String,
    pub status: HealthStatus,
    pub issue: Option<String>,
}

/// The publication protocol currently records no failed-attempt coordinates.
/// This type makes the empty evidence set explicit rather than inferring
/// candidates from an object listing.
#[derive(Debug, Clone, Serialize)]
pub struct FailedAttemptCandidate {
    pub evidence: String,
}
