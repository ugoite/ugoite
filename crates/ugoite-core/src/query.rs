//! Domain policy for a closed, authorized analytical query surface.
//!
//! Core decides which logical Forms, Entry IDs, columns, functions, snapshots,
//! and resources a caller may use. Storage adapters translate this DTO into
//! their query engine without exposing physical provider or catalog types.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use ugoite_domain::checkpoint::SpaceCheckpoint;
use ugoite_domain::id::{EntryId, FormId};

use crate::error::{AppError, ErrorCode};

/// Maximum keyword query size in bytes. This is the shared Search admission
/// bound: every Search entry point (core service, server handler, CLI) must
/// reject larger input before touching Storage, DataFusion, or derived search.
pub const MAX_SEARCH_QUERY_BYTES: usize = 8 * 1024;

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
    Tags,
    CreatedAt,
    UpdatedAt,
    EntryId,
    EntryVersion,
    CommittedAt,
    RevisionId,
    ParentRevisionId,
    Author,
    UpdatedBy,
    DeletedBy,
    ExtraAttributes,
    Integrity,
    Deleted,
    DeletedAt,
}

impl QuerySystemColumn {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExternalId => "_ugoite_id",
            Self::Tags => "_ugoite_tags",
            Self::CreatedAt => "_ugoite_created_at",
            Self::UpdatedAt => "_ugoite_updated_at",
            Self::EntryId => "_ugoite_entry_id",
            Self::EntryVersion => "_ugoite_entry_version",
            Self::CommittedAt => "_ugoite_committed_at",
            Self::RevisionId => "_ugoite_revision_id",
            Self::ParentRevisionId => "_ugoite_parent_revision_id",
            Self::Author => "_ugoite_author",
            Self::UpdatedBy => "_ugoite_updated_by",
            Self::DeletedBy => "_ugoite_deleted_by",
            Self::ExtraAttributes => "_ugoite_extra_attributes",
            Self::Integrity => "_ugoite_integrity",
            Self::Deleted => "_ugoite_deleted",
            Self::DeletedAt => "_ugoite_deleted_at",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryLimits {
    pub max_memory_bytes: usize,
    pub max_rows: usize,
    pub timeout: Duration,
    pub max_concurrency: usize,
    /// Function names explicitly admitted by Core. Callers cannot provide
    /// UDF implementations; a storage adapter may register a fixed,
    /// internally-owned function only for an adapter-owned derived plan.
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

/// Shared Search admission: an empty or whitespace-only keyword is not a valid
/// Ugoite operation. Callers must reject it before any broad scan, Storage
/// read, or derived-search fan-out.
pub fn validate_keyword_query(query: &str) -> Result<(), AppError> {
    if query.trim().is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::SearchQueryEmpty,
            "search query must not be empty",
        ));
    }
    if query.len() > MAX_SEARCH_QUERY_BYTES {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "search query exceeds the configured byte limit",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_whitespace_queries_are_rejected_before_search() {
        for query in ["", "   ", "\n\t "] {
            let error = validate_keyword_query(query).expect_err("empty query must fail");
            assert_eq!(error.code(), ErrorCode::SearchQueryEmpty);
            assert_eq!(error.kind(), crate::error::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn oversized_query_is_rejected_at_admission() {
        let error = validate_keyword_query(&"x".repeat(MAX_SEARCH_QUERY_BYTES + 1))
            .expect_err("oversized query must fail");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn normal_query_is_accepted() {
        assert!(validate_keyword_query("hello").is_ok());
        assert!(validate_keyword_query(&"x".repeat(MAX_SEARCH_QUERY_BYTES)).is_ok());
    }
}
