//! Domain policy for a closed, authorized analytical query surface.
//!
//! Core decides which logical Forms, Entry IDs, columns, functions, snapshots,
//! and resources a caller may use. Storage adapters translate this DTO into
//! their query engine without exposing physical provider or catalog types.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use ugoite_domain::id::{EntryId, FormId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedQueryPolicy {
    pub forms: BTreeMap<FormId, AuthorizedQueryForm>,
    pub readable_entry_ids: BTreeSet<EntryId>,
    pub checkpoint: Option<QueryCheckpoint>,
    pub limits: QueryLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedQueryForm {
    /// The sole SQL relation name exposed for this Form.
    pub relation: String,
    /// User-defined Form columns which may be resolved or projected.
    pub columns: BTreeSet<String>,
    /// System columns which are intentionally part of this query contract.
    pub system_columns: BTreeSet<QuerySystemColumn>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryCheckpoint {
    /// Every exposed Form must have a snapshot in this map. A partial
    /// checkpoint is rejected by the physical adapter rather than mixing live
    /// and historical providers.
    pub form_snapshots: BTreeMap<FormId, i64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuerySystemColumn {
    EntryId,
    EntryVersion,
    CommittedAt,
}

impl QuerySystemColumn {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EntryId => "entry_id",
            Self::EntryVersion => "entry_version",
            Self::CommittedAt => "committed_at",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryLimits {
    pub max_memory_bytes: usize,
    pub max_rows: usize,
    pub timeout: Duration,
    pub max_concurrency: usize,
    /// DataFusion built-in function names explicitly admitted by Core. UDFs
    /// are never registered by the authorized query surface.
    pub allowed_functions: BTreeSet<String>,
}

impl QueryLimits {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.max_memory_bytes == 0 {
            return Err("query memory limit must be positive");
        }
        if self.max_rows == 0 {
            return Err("query row limit must be positive");
        }
        if self.timeout.is_zero() {
            return Err("query timeout must be positive");
        }
        if self.max_concurrency == 0 {
            return Err("query concurrency limit must be positive");
        }
        Ok(())
    }
}
