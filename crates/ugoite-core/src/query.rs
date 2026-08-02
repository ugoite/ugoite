//! Domain policy for a closed, authorized analytical query surface.
//!
//! Core decides which logical Forms, Entry IDs, columns, functions, snapshots,
//! and resources a caller may use. Storage adapters translate this DTO into
//! their query engine without exposing physical provider or catalog types.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use ugoite_domain::checkpoint::SpaceCheckpoint;
use ugoite_domain::id::{EntryId, FormId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedQueryPolicy {
    pub forms: BTreeMap<FormId, AuthorizedQueryForm>,
    /// When present, every provider is built from this one complete,
    /// publication-verified Space coordinate.
    pub checkpoint: Option<SpaceCheckpoint>,
    pub limits: QueryLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedQueryForm {
    /// The sole SQL relation name exposed for this Form.
    pub relation: String,
    /// Entry scope Core authorizes for this Form. The query adapter embeds this
    /// relation-specific boundary in the trusted view before SQL is planned.
    pub entry_scope: EntryScope,
    /// Backend-owned stable SQL columns which may be resolved or projected.
    pub columns: BTreeSet<String>,
    /// System columns which are intentionally part of this query contract.
    pub system_columns: BTreeSet<QuerySystemColumn>,
}

/// The Entry set that a relation may expose. Core can authorize the whole Form
/// without first materializing its current Entries into Rust; remote callers
/// use either an explicit allow-list or sparse Entry-level exceptions supplied
/// by the authorization boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryScope {
    AllCurrent,
    Only(BTreeSet<EntryId>),
    /// Exposes the current Form without materializing every permitted Entry
    /// ID in Core. The trusted DataFusion view removes the listed exceptions
    /// before it derives each Entry's latest revision.
    AllExcept(BTreeSet<EntryId>),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuerySystemColumn {
    ExternalId,
    Title,
    Tags,
    CreatedAt,
    UpdatedAt,
    EntryId,
    EntryVersion,
    CommittedAt,
}

impl QuerySystemColumn {
    pub const fn as_str(self) -> &'static str {
        match self {
            // Form fields own the ordinary SQL namespace. Stable Entry
            // metadata is deliberately namespaced so an otherwise valid
            // Form with a `title`, `id`, or timestamp field can never make a
            // query context fail at runtime.
            Self::ExternalId => "_ugoite_id",
            Self::Title => "_ugoite_title",
            Self::Tags => "_ugoite_tags",
            Self::CreatedAt => "_ugoite_created_at",
            Self::UpdatedAt => "_ugoite_updated_at",
            Self::EntryId => "_ugoite_entry_id",
            Self::EntryVersion => "_ugoite_entry_version",
            Self::CommittedAt => "_ugoite_committed_at",
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
