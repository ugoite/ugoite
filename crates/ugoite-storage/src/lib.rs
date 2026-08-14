//! Persistence adapter boundary for Ugoite.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::TryStreamExt;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::services::{Fs, Memory, S3};
use opendal::{EntryMode, ErrorKind, Operator};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

pub use ugoite_domain as domain;

/// Normalized backend information used by the Iceberg OpenDAL adapter.
///
/// It deliberately contains no `Operator`: storage owns the live operator used
/// for Catalog Head operations, while Iceberg receives only the configuration
/// it needs to build its official OpenDAL storage factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcebergStorageConfig {
    pub scheme: String,
    pub warehouse_uri: String,
    pub properties: HashMap<String, String>,
}

impl IcebergStorageConfig {
    pub fn from_operator(operator: &Operator) -> Result<Self> {
        let info = operator.info();
        let scheme = info.scheme().to_string();
        let operator_root = info.root();
        let root = operator_root.trim_end_matches('/');
        let warehouse_uri = match scheme.as_str() {
            "fs" | "file" => format!("file://{}", if root.is_empty() { "/" } else { root }),
            "memory" => "memory:///".to_string(),
            "s3" => format!("s3://{}/{}", info.name(), root.trim_start_matches('/')),
            "gcs" => format!("gs://{}/{}", info.name(), root.trim_start_matches('/')),
            "oss" => format!("oss://{}/{}", info.name(), root.trim_start_matches('/')),
            "azdls" => format!("abfs://{}/{}", info.name(), root.trim_start_matches('/')),
            unsupported => {
                return Err(anyhow!("unsupported Iceberg storage scheme: {unsupported}"))
            }
        };
        Ok(Self {
            scheme,
            warehouse_uri: warehouse_uri.trim_end_matches('/').to_string(),
            properties: HashMap::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactCatalogHead {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogWriteMode {
    Shared,
    SingleProcess,
}

/// Minimal durable coordinate published by one rebuildable relation. Iceberg
/// metadata owns the table details; this document only binds the visible
/// current build. Previous builds are garbage-collection candidates, not
/// relation history.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedRelationHead {
    pub format_version: u32,
    pub space_id: String,
    pub relation_id: String,
    pub generation: u64,
    pub definition_version: u32,
    pub definition_fingerprint: String,
    pub producer_id: String,
    pub producer_fingerprint: String,
    pub compatibility_epoch: u64,
    pub build_id: String,
    pub table_identifier: serde_json::Value,
    pub table_uuid: String,
    pub metadata_location: String,
    pub snapshot_id: Option<i64>,
    pub schema_id: i32,
    pub input_digest: String,
    pub source_coordinate: serde_json::Value,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactDerivedRelationHead {
    pub head: DerivedRelationHead,
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

const DERIVED_BUILD_CLAIM_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub struct LegacyDerivedRelationHead;

impl std::fmt::Display for LegacyDerivedRelationHead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("legacy DerivedRelation Head format")
    }
}

impl std::error::Error for LegacyDerivedRelationHead {}

/// Relation-local Head storage. It deliberately shares only OpenDAL object
/// mechanics with the authoritative Catalog store and has no Catalog history
/// or checkpoint behavior.
#[derive(Clone)]
pub struct DerivedRelationHeadStore {
    operator: Operator,
    space_root: String,
    relation_id: Uuid,
    write_mode: CatalogWriteMode,
    serializer: Arc<AsyncMutex<()>>,
}

impl std::fmt::Debug for DerivedRelationHeadStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DerivedRelationHeadStore")
            .field("space_root", &self.space_root)
            .field("relation_id", &self.relation_id)
            .field("write_mode", &self.write_mode)
            .finish()
    }
}

impl DerivedRelationHeadStore {
    pub fn new(operator: Operator, space_root: impl Into<String>, relation_id: Uuid) -> Self {
        let space_root = space_root.into().trim_matches('/').to_string();
        let serializer =
            catalog_serializer(&operator, &format!("{space_root}/derived/{relation_id}"));
        Self {
            operator,
            space_root,
            relation_id,
            write_mode: CatalogWriteMode::SingleProcess,
            serializer,
        }
    }

    pub async fn shared(mut self) -> Result<Self> {
        // Reuse the existing full OpenDAL contract probe.  Capability bits
        // alone are insufficient: a backend must prove changing ETags,
        // exact reads, create-if-absent, if-match replacement, and stale
        // rejection before shared Relation Head writes are admitted.
        SpaceCatalogStore::new(self.operator.clone(), self.space_root.clone())?
            .verify_shared_writes()
            .await?;
        self.write_mode = CatalogWriteMode::Shared;
        Ok(self)
    }

    pub fn single_process(mut self) -> Self {
        self.write_mode = CatalogWriteMode::SingleProcess;
        self
    }

    pub fn write_mode(&self) -> CatalogWriteMode {
        self.write_mode
    }

    pub fn head_path(&self) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/head.json",
            self.space_root, self.relation_id
        )
    }

    pub fn builds_path(&self, build_id: &str) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/builds/{build_id}",
            self.space_root, self.relation_id
        )
    }

    fn legacy_materializations_prefix(&self) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/materializations/",
            self.space_root, self.relation_id
        )
    }

    fn garbage_marker_path(&self, build_id: &str) -> String {
        format!("{}/garbage.json", self.builds_path(build_id))
    }

    fn staging_marker_path(&self, build_id: &str) -> String {
        format!("{}/staging.json", self.builds_path(build_id))
    }

    /// Create the marker before any immutable build object is written.  A
    /// failed marker write aborts staging before it can leave an unidentifiable
    /// prefix behind.
    pub async fn mark_staging(&self, build_id: &str) -> Result<()> {
        self.operator
            .write_options(
                &self.staging_marker_path(build_id),
                Self::build_marker_bytes(),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    pub async fn clear_staging(&self, build_id: &str) -> Result<()> {
        match self
            .operator
            .delete(&self.staging_marker_path(build_id))
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn publishing_marker_path(&self, build_id: &str) -> String {
        format!("{}/publishing.json", self.builds_path(build_id))
    }

    fn build_claim_bytes(build_id: &str, role: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "build_id": build_id,
            "role": role,
            "claimed_at": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }))
        .expect("derived build claim is serializable")
    }

    async fn read_build_claim(
        &self,
        build_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<String>, Option<SystemTime>)>> {
        let path = self.publishing_marker_path(build_id);
        let metadata = match self.operator.stat(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let etag = metadata
            .etag()
            .filter(|etag| !etag.is_empty())
            .map(str::to_owned);
        let bytes = match etag.as_deref() {
            Some(etag) => {
                self.operator
                    .read_options(
                        &path,
                        ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
            }
            None => self.operator.read(&path).await,
        }?;
        Ok(Some((
            bytes.to_vec(),
            etag,
            metadata.last_modified().map(|timestamp| timestamp.into()),
        )))
    }

    fn claim_is_stale(bytes: &[u8], last_modified: Option<SystemTime>) -> bool {
        Self::json_time(bytes, "claimed_at")
            .or(last_modified)
            .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
            .is_some_and(|age| age >= DERIVED_BUILD_CLAIM_TTL)
    }

    fn build_marker_bytes() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "marked_at": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }))
        .expect("derived build marker is serializable")
    }

    fn json_time(bytes: &[u8], key: &str) -> Option<SystemTime> {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| value.get(key).and_then(serde_json::Value::as_u64))
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
    }

    fn marker_time(bytes: &[u8]) -> Option<SystemTime> {
        Self::json_time(bytes, "marked_at")
    }

    async fn marker_time_or_metadata(
        &self,
        path: &str,
        metadata_time: Option<SystemTime>,
    ) -> Option<SystemTime> {
        self.operator
            .read(path)
            .await
            .ok()
            .and_then(|bytes| Self::marker_time(&bytes.to_vec()))
            .or(metadata_time)
    }

    fn claim_role(bytes: &[u8]) -> Option<String> {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
    }

    async fn replace_build_claim(
        &self,
        build_id: &str,
        expected_etag: Option<&str>,
        role: &str,
    ) -> Result<bool> {
        let path = self.publishing_marker_path(build_id);
        let result = match (self.write_mode, expected_etag) {
            (CatalogWriteMode::Shared, Some(etag)) => {
                self.operator
                    .write_options(
                        &path,
                        Self::build_claim_bytes(build_id, role),
                        WriteOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
            }
            (CatalogWriteMode::SingleProcess, _) => {
                self.operator
                    .write(&path, Self::build_claim_bytes(build_id, role))
                    .await
            }
            (CatalogWriteMode::Shared, None) => {
                return Err(anyhow!(
                    "shared DerivedRelation build claim did not return an ETag"
                ));
            }
        };
        match result {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    async fn begin_publishing(&self, build_id: &str) -> Result<()> {
        let path = self.publishing_marker_path(build_id);
        let result = self
            .operator
            .write_options(
                &path,
                Self::build_claim_bytes(build_id, "publishing"),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await;
        if let Err(error) = result {
            if error.kind() != ErrorKind::ConditionNotMatch {
                return Err(error.into());
            }
            let Some((bytes, etag, last_modified)) = self.read_build_claim(build_id).await? else {
                return Err(anyhow!("DerivedRelation build claim disappeared"));
            };
            // A garbage collector owns a reclaimed build permanently. A
            // publisher must never take that claim back after its lease age,
            // because GC may already be deleting the immutable prefix.
            if Self::claim_role(&bytes).as_deref() != Some("publishing") {
                return Err(anyhow!("DerivedRelation build claim is held"));
            }
            if !Self::claim_is_stale(&bytes, last_modified)
                || !self
                    .replace_build_claim(build_id, etag.as_deref(), "publishing")
                    .await?
            {
                return Err(anyhow!("DerivedRelation build claim is held"));
            }
        }
        self.ensure_build_publishable(build_id).await?;
        Ok(())
    }

    /// Claims the same durable marker used by publication before GC writes a
    /// garbage marker or deletes any build object. The if-match replacement
    /// is the shared-backend exclusion primitive: either publication owns the
    /// marker, or GC owns it, never both.
    async fn claim_build_for_garbage(&self, build_id: &str) -> Result<bool> {
        let path = self.publishing_marker_path(build_id);
        match self
            .operator
            .write_options(
                &path,
                Self::build_claim_bytes(build_id, "garbage"),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => {
                let Some((bytes, etag, last_modified)) = self.read_build_claim(build_id).await?
                else {
                    return Ok(false);
                };
                if !matches!(
                    Self::claim_role(&bytes).as_deref(),
                    Some("publishing") | Some("garbage")
                ) {
                    return Ok(false);
                }
                if !Self::claim_is_stale(&bytes, last_modified) {
                    return Ok(false);
                }
                self.replace_build_claim(build_id, etag.as_deref(), "garbage")
                    .await
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Refresh the garbage claim before each destructive object operation.
    /// This keeps a long-running deletion from becoming an apparently stale
    /// claim and fences publication from a reclaimed build.
    async fn renew_garbage_claim(&self, build_id: &str) -> Result<bool> {
        let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        if Self::claim_role(&bytes).as_deref() != Some("garbage") {
            return Ok(false);
        }
        self.replace_build_claim(build_id, etag.as_deref(), "garbage")
            .await
    }

    /// Mark a build at the moment it stops being current.  GC uses this
    /// marker's timestamp rather than the build's creation timestamp so an
    /// old, long-lived current build still gets a full reader grace period
    /// after the Head swap.
    pub async fn mark_garbage(&self, build_id: &str) -> Result<()> {
        match self
            .operator
            .write_options(
                &self.garbage_marker_path(build_id),
                Self::build_marker_bytes(),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            // A concurrent publisher/GC may have already made the marker
            // durable. Marker creation is an idempotent lifecycle transition.
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn clear_garbage(&self, build_id: &str) -> Result<()> {
        match self
            .operator
            .delete(&self.garbage_marker_path(build_id))
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    async fn ensure_build_publishable(&self, build_id: &str) -> Result<()> {
        if self
            .operator
            .exists(&self.garbage_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build is marked garbage"));
        }
        Ok(())
    }

    /// The relation-local mutex covers an entire single-process rebuild. Head
    /// CAS alone is intentionally insufficient: two local builders must not
    /// scan, parse, and publish concurrently for the same relation.
    pub fn single_process_lock(&self) -> Arc<AsyncMutex<()>> {
        self.serializer.clone()
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    /// Remove non-current build prefixes after the caller-selected grace
    /// period. Listing discovers explicit markers and sufficiently old
    /// marker-less orphan prefixes; Head is the sole authority for the current
    /// build.
    pub async fn garbage_collect(
        &self,
        current_build_id: Option<&str>,
        minimum_gc_age: Duration,
    ) -> Result<Vec<String>> {
        if self.write_mode == CatalogWriteMode::SingleProcess {
            let _guard = self.serializer.lock().await;
            return self
                .garbage_collect_with_single_process_lock(current_build_id, minimum_gc_age)
                .await;
        }
        self.garbage_collect_with_single_process_lock(current_build_id, minimum_gc_age)
            .await
    }

    /// Runs GC while the caller already owns the relation-local single-process
    /// lock. Rebuild publication uses this variant so the GC scan/delete
    /// cannot interleave with its final Head swap.
    pub async fn garbage_collect_with_single_process_lock(
        &self,
        current_build_id: Option<&str>,
        minimum_gc_age: Duration,
    ) -> Result<Vec<String>> {
        let prefix = format!(
            "{}/_ugoite/derived/relations/{}/builds/",
            self.space_root, self.relation_id
        );
        let entries = self.operator.list_with(&prefix).recursive(true).await?;
        #[derive(Default)]
        struct Candidate {
            garbage_marker_old_enough: bool,
            stale_staging_old_enough: bool,
            has_garbage_marker: bool,
            has_staging_marker: bool,
            has_publishing_marker: bool,
            stale_publishing_old_enough: bool,
            newest_object_modified: Option<SystemTime>,
            orphan_old_enough: bool,
        }

        let mut candidates = std::collections::BTreeMap::<String, Candidate>::new();
        for entry in entries {
            if entry.metadata().mode() != EntryMode::FILE {
                continue;
            }
            let Some(build_id) = entry
                .path()
                .strip_prefix(&prefix)
                .and_then(|path| path.split('/').next())
                .filter(|build_id| !build_id.is_empty())
            else {
                continue;
            };
            if Some(build_id) == current_build_id {
                continue;
            }
            let is_garbage_marker = entry.path() == self.garbage_marker_path(build_id);
            let is_staging_marker = entry.path() == self.staging_marker_path(build_id);
            let is_publishing_marker = entry.path() == self.publishing_marker_path(build_id);
            let modified = entry.metadata().last_modified().map(Into::into);
            let marker_modified = if is_garbage_marker || is_staging_marker {
                self.marker_time_or_metadata(entry.path(), modified).await
            } else {
                modified
            };
            let age = marker_modified
                .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok());
            let old_enough =
                minimum_gc_age.is_zero() || age.is_some_and(|age| age >= minimum_gc_age);
            let candidate = candidates.entry(build_id.to_string()).or_default();
            if is_garbage_marker {
                candidate.has_garbage_marker = true;
                candidate.garbage_marker_old_enough |= old_enough;
            }
            if is_staging_marker {
                candidate.has_staging_marker = true;
                candidate.stale_staging_old_enough |= old_enough;
            }
            if is_publishing_marker {
                candidate.has_publishing_marker = true;
            }
            if let Some(modified) = modified {
                candidate.newest_object_modified = Some(
                    candidate
                        .newest_object_modified
                        .map_or(modified, |current| current.max(modified)),
                );
            }
        }
        for (build_id, candidate) in &mut candidates {
            // A crash after publication claim creation can leave only
            // publishing.json behind. A live claim protects the build, while
            // a stale publishing claim is recoverable cleanup intent. A
            // terminal garbage claim remains a tombstone and is never
            // reclaimed.
            if candidate.has_publishing_marker {
                if let Some((bytes, _, last_modified)) = self.read_build_claim(build_id).await? {
                    candidate.stale_publishing_old_enough = Self::claim_role(&bytes).as_deref()
                        == Some("publishing")
                        && Self::claim_is_stale(&bytes, last_modified);
                }
            }
            // A marker-less prefix can be left behind by a crash immediately
            // after Head CAS and before the superseded build is marked garbage.
            // Only consider it after every object has been quiet for the full
            // grace period, and never while a staging or live publishing claim
            // is present. A stale publishing claim is handled separately as
            // recoverable cleanup intent. Head remains the authority for the
            // final deletion check.
            candidate.orphan_old_enough = !candidate.has_garbage_marker
                && !candidate.has_staging_marker
                && !candidate.has_publishing_marker
                && (candidate.newest_object_modified.is_none() && minimum_gc_age.is_zero()
                    || candidate.newest_object_modified.is_some_and(|modified| {
                        minimum_gc_age.is_zero()
                            || SystemTime::now()
                                .duration_since(modified)
                                .is_ok_and(|age| age >= minimum_gc_age)
                    }));
        }
        let mut deleted = Vec::new();
        for (build_id, candidate) in candidates {
            // A garbage marker is written after a build has either lost
            // publication or stopped being current. A stale staging marker is
            // also a durable cleanup candidate: it covers a process crash
            // between staging and the failure path that writes garbage.json.
            // Once garbage.json exists it is the cleanup record for this
            // build. Its own age is the grace-period boundary; an older
            // staging marker must not allow a freshly marked build to be
            // reclaimed early.
            let cleanup_old_enough = if candidate.has_garbage_marker {
                candidate.garbage_marker_old_enough
            } else {
                candidate.stale_staging_old_enough
                    || candidate.orphan_old_enough
                    || candidate.stale_publishing_old_enough
            };
            if !cleanup_old_enough {
                continue;
            }
            // GC is discovery-only and must never decide authority from the
            // listing. Re-read the durable Head immediately before deleting
            // each candidate so a concurrent shared publisher is protected.
            if self
                .read_exact()
                .await?
                .as_ref()
                .is_some_and(|head| head.head.build_id == build_id)
            {
                continue;
            }
            let needs_fresh_garbage_marker = !candidate.has_garbage_marker
                && (candidate.stale_staging_old_enough
                    || candidate.orphan_old_enough
                    || candidate.stale_publishing_old_enough);
            if needs_fresh_garbage_marker {
                // The first pass only records cleanup intent. This makes the
                // marker timestamp the grace-period boundary even when an
                // old staging/publishing object is being reclaimed.
                self.mark_garbage(&build_id).await?;
                if self
                    .read_exact()
                    .await?
                    .as_ref()
                    .is_some_and(|head| head.head.build_id == build_id)
                {
                    let _ = self.clear_garbage(&build_id).await;
                    continue;
                }
                // Markerless orphans have no durable cleanup timestamp, so a
                // zero-age maintenance pass may claim and delete them in one
                // pass. Staging/publishing recovery must always defer after
                // recording garbage.json: its timestamp is the reader grace
                // boundary, even when the caller explicitly selected zero.
                if !(candidate.orphan_old_enough && minimum_gc_age.is_zero()) {
                    continue;
                }
            }
            // Publication and GC claim the same object with conditional
            // create/replace. A fresh claim belongs to the other operation;
            // a stale claim can be atomically taken over for recovery.
            if !self.claim_build_for_garbage(&build_id).await? {
                continue;
            }
            let build_prefix = self.builds_path(&build_id);
            let entries = self
                .operator
                .list_with(&build_prefix)
                .recursive(true)
                .await?;
            let mut garbage_marker = None;
            let mut build_objects = Vec::new();
            for entry in entries {
                if entry.metadata().mode() != EntryMode::FILE {
                    continue;
                }
                if entry.path() == self.garbage_marker_path(&build_id) {
                    // The marker is the durable discovery record. It must be
                    // removed only after every other object has been deleted,
                    // so a crash during cleanup leaves the build discoverable
                    // on the next GC pass.
                    garbage_marker = Some(entry.path().to_string());
                } else if entry.path() == self.publishing_marker_path(&build_id) {
                    // A garbage claim is also the terminal tombstone.  It
                    // must remain after cleanup: a publisher that was paused
                    // before its claim attempt can otherwise create a fresh
                    // claim after garbage.json is removed and publish a Head
                    // for this already-deleted build.
                } else {
                    build_objects.push(entry.path().to_string());
                }
            }
            let mut fully_deleted = true;
            for path in build_objects {
                if !self.renew_garbage_claim(&build_id).await? {
                    fully_deleted = false;
                    break;
                }
                // Re-check the Head for every object as a fail-closed guard
                // against a concurrent publication.
                if self
                    .read_exact()
                    .await?
                    .as_ref()
                    .is_some_and(|head| head.head.build_id == build_id)
                {
                    fully_deleted = false;
                    break;
                }
                self.operator.delete(&path).await?;
            }
            if fully_deleted {
                // Keep garbage.json until the build prefix is otherwise empty.
                // If this process crashes before this final delete, the marker
                // remains available for the next candidate-discovery pass.
                if self
                    .read_exact()
                    .await?
                    .as_ref()
                    .is_some_and(|head| head.head.build_id == build_id)
                {
                    continue;
                }
                // The garbage marker is the final durable cleanup record. If
                // a publisher won the Head CAS after the previous check, the
                // marker must remain so the build is rediscovered safely. The
                // garbage claim is intentionally retained as a terminal
                // tombstone, fencing delayed publishers even after this
                // marker is removed.
                if self
                    .read_exact()
                    .await?
                    .as_ref()
                    .is_some_and(|head| head.head.build_id == build_id)
                {
                    continue;
                }
                if let Some(path) = garbage_marker {
                    self.operator.delete(&path).await?;
                }
                deleted.push(build_id);
            }
        }
        Ok(deleted)
    }

    pub async fn read_exact(&self) -> Result<Option<ExactDerivedRelationHead>> {
        for attempt in 0..3 {
            let metadata = match self.operator.stat(&self.head_path()).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let etag = metadata
                .etag()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let read = match etag.as_deref() {
                Some(etag) => {
                    self.operator
                        .read_options(
                            &self.head_path(),
                            ReadOptions {
                                if_match: Some(etag.to_string()),
                                ..Default::default()
                            },
                        )
                        .await
                }
                None if self.write_mode == CatalogWriteMode::Shared => {
                    return Err(anyhow!(
                        "exact DerivedRelation Head stat did not return an ETag"
                    ))
                }
                None => self.operator.read(&self.head_path()).await,
            };
            match read {
                Ok(bytes) => {
                    let bytes = bytes.to_vec();
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).context("decode DerivedRelation Head")?;
                    let head: DerivedRelationHead =
                        serde_json::from_value(value.clone()).map_err(|error| {
                            if is_legacy_derived_head(&value) {
                                LegacyDerivedRelationHead.into()
                            } else {
                                anyhow!("decode DerivedRelation Head: {error}")
                            }
                        })?;
                    validate_derived_head_checksum(&head)?;
                    return Ok(Some(ExactDerivedRelationHead { head, bytes, etag }));
                }
                Err(error) if error.kind() == ErrorKind::ConditionNotMatch && attempt < 2 => {
                    continue
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact derived Head read attempts always return or continue")
    }

    /// v1 is intentionally not kept as an active compatibility format: its
    /// Head points at the removed materializations/manifest layout.  A local
    /// rebuild may explicitly invalidate that derived-only Head and recreate
    /// the relation under the current-build layout. Shared mode fails closed
    /// because OpenDAL has no conditional delete operation.
    pub async fn invalidate_legacy_head(&self) -> Result<()> {
        if self.write_mode == CatalogWriteMode::Shared {
            return Err(anyhow!(
                "legacy DerivedRelation Head requires a single-process rebuild"
            ));
        }
        let _guard = self.serializer.lock().await;
        let Some(metadata) = self.operator.stat(&self.head_path()).await.ok() else {
            return Ok(());
        };
        let etag = metadata
            .etag()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let bytes = match etag.as_deref() {
            Some(etag) => {
                self.operator
                    .read_options(
                        &self.head_path(),
                        ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await?
            }
            None => self.operator.read(&self.head_path()).await?,
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;
        if is_legacy_derived_head(&value) {
            // Remove the disposable legacy prefix first. If listing or any
            // delete fails, keep the legacy Head so the next rebuild retries
            // the cleanup instead of losing the only migration signal.
            let entries = self
                .operator
                .list_with(&self.legacy_materializations_prefix())
                .recursive(true)
                .await?;
            for entry in entries {
                if entry.metadata().mode() == EntryMode::FILE {
                    self.operator.delete(entry.path()).await?;
                }
            }
            self.operator.delete(&self.head_path()).await?;
        }
        Ok(())
    }

    pub async fn create(&self, head: &DerivedRelationHead) -> Result<()> {
        self.ensure_build_publishable(&head.build_id).await?;
        let bytes = canonical_head_bytes(head)?;
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_not_exists: true,
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                let _guard = self.serializer.lock().await;
                if self.operator.exists(&self.head_path()).await? {
                    return Err(anyhow!("DerivedRelation Head already exists"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn replace(
        &self,
        expected_etag: Option<&str>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        self.ensure_build_publishable(&head.build_id).await?;
        let bytes = canonical_head_bytes(head)?;
        match self.write_mode {
            CatalogWriteMode::Shared => {
                let etag =
                    expected_etag.context("shared DerivedRelation replacement requires an ETag")?;
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                let _guard = self.serializer.lock().await;
                let current = self
                    .read_exact()
                    .await?
                    .context("DerivedRelation Head disappeared")?;
                if expected_etag.is_some() && current.etag.as_deref() != expected_etag {
                    return Err(anyhow!("DerivedRelation Head changed"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn publish(
        &self,
        expected: Option<&ExactDerivedRelationHead>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        if self.write_mode == CatalogWriteMode::SingleProcess {
            let _guard = self.serializer.lock().await;
            return self.publish_with_single_process_lock(expected, head).await;
        }
        self.begin_publishing(&head.build_id).await?;
        match expected {
            None => self.create(head).await,
            Some(expected) => self.replace(expected.etag.as_deref(), head).await,
        }
    }

    /// Publish while the caller already owns [`Self::single_process_lock`].
    /// This is used by a full rebuild so the relation mutex spans source scan,
    /// build, validation, and swap without self-deadlocking on Head I/O.
    pub async fn publish_with_single_process_lock(
        &self,
        expected: Option<&ExactDerivedRelationHead>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        self.begin_publishing(&head.build_id).await?;
        if self.write_mode != CatalogWriteMode::SingleProcess {
            return match expected {
                None => self.create(head).await,
                Some(expected) => self.replace(expected.etag.as_deref(), head).await,
            };
        }
        let bytes = canonical_head_bytes(head)?;
        match expected {
            None => {
                if self.operator.exists(&self.head_path()).await? {
                    return Err(anyhow!("DerivedRelation Head already exists"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
            Some(expected) => {
                let current = self
                    .read_exact()
                    .await?
                    .context("DerivedRelation Head disappeared")?;
                if (expected.etag.is_some() && current.etag != expected.etag)
                    || (expected.etag.is_none() && current.bytes != expected.bytes)
                {
                    return Err(anyhow!("DerivedRelation Head changed"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }
}

fn canonical_head_bytes(head: &DerivedRelationHead) -> Result<Vec<u8>> {
    let mut canonical = head.clone();
    canonical.checksum.clear();
    canonical.checksum = ugoite_domain::derived_relation::sha256_digest(
        &serde_json::to_vec(&canonical).context("canonicalize DerivedRelation Head")?,
    );
    serde_json::to_vec(&canonical).context("encode DerivedRelation Head")
}

fn validate_derived_head_checksum(head: &DerivedRelationHead) -> Result<()> {
    let mut canonical = head.clone();
    let observed = canonical.checksum.clone();
    canonical.checksum.clear();
    let expected = ugoite_domain::derived_relation::sha256_digest(
        &serde_json::to_vec(&canonical).context("canonicalize DerivedRelation Head")?,
    );
    if observed != expected {
        return Err(anyhow!("DerivedRelation Head checksum mismatch"));
    }
    Ok(())
}

fn is_legacy_derived_head(value: &serde_json::Value) -> bool {
    value.get("materialization_id").is_some()
        || value.get("base_generation").is_some()
        || value.get("target_generation").is_some()
        || value.get("materialization_manifest_location").is_some()
        || value.get("last_command_id").is_some()
}

/// Read-only declaration of the OpenDAL contract backing a Space Catalog.
/// It deliberately reports only capability bits and whether shared mode was
/// previously admitted; it never triggers a write probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogBackendCapabilities {
    pub etag: bool,
    pub read_with_if_match: bool,
    pub write_with_if_match: bool,
    pub write_with_if_not_exists: bool,
    pub shared_write_contract: bool,
}

const EXACT_HEAD_READ_ATTEMPTS: usize = 3;

/// Narrow OpenDAL boundary for the Space Catalog root and immutable
/// publication evidence. It is intentionally not a general-purpose wrapper.
#[derive(Clone)]
pub struct SpaceCatalogStore {
    operator: Operator,
    space_root: String,
    storage: IcebergStorageConfig,
    write_mode: CatalogWriteMode,
    single_process_serializer: Arc<AsyncMutex<()>>,
    read_counter: Option<Arc<AtomicUsize>>,
}

impl std::fmt::Debug for SpaceCatalogStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpaceCatalogStore")
            .field("space_root", &self.space_root)
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

impl SpaceCatalogStore {
    pub fn new(operator: Operator, space_root: impl Into<String>) -> Result<Self> {
        let space_root = space_root.into().trim_matches('/').to_string();
        let single_process_serializer = catalog_serializer(&operator, &space_root);
        Ok(Self {
            storage: IcebergStorageConfig::from_operator(&operator)?,
            operator,
            space_root,
            // Shared mode is opt-in only after `verify_shared_writes` proves
            // the actual backend honors every conditional operation we need.
            write_mode: CatalogWriteMode::SingleProcess,
            single_process_serializer,
            read_counter: None,
        })
    }

    /// Attaches a logical object-read counter for deterministic storage
    /// instrumentation in Catalog scalability tests. The counter is not part
    /// of the production coordination protocol.
    pub fn with_read_counter(mut self) -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        self.read_counter = Some(counter.clone());
        (self, counter)
    }

    pub fn single_process(mut self) -> Self {
        self.write_mode = CatalogWriteMode::SingleProcess;
        self
    }

    pub fn write_mode(&self) -> CatalogWriteMode {
        self.write_mode
    }

    /// A process-local serializer, used only in explicit single-process mode.
    /// It is not a cross-process lock and does not participate in shared CAS.
    pub fn single_process_serializer(&self) -> Arc<AsyncMutex<()>> {
        self.single_process_serializer.clone()
    }

    /// Proves the configured persistent backend supports the exact conditional
    /// sequence required by shared Catalog Head publication, then enables
    /// shared mode for this store value. The immutable probe is evidence only;
    /// it is never used for recovery or coordination.
    pub async fn verify_shared_writes(mut self) -> Result<Self> {
        if !self.supports_shared_writes() {
            return Err(anyhow!(
                "shared Catalog writes require ETag-bound reads and conditional writes"
            ));
        }
        let cache_key = (self.operator.info().scheme() != "memory")
            .then(|| self.shared_write_verification_key());
        if let Some(cache_key) = &cache_key {
            if SHARED_WRITE_VERIFICATIONS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .expect("shared-write verification cache poisoned")
                .iter()
                .any(|(key, _)| key == cache_key)
            {
                self.write_mode = CatalogWriteMode::Shared;
                return Ok(self);
            }
        }
        let path = self.catalog_path(&format!("probes/{}.json", Uuid::now_v7()));
        let initial = b"{\"format_version\":1,\"stage\":\"created\"}".to_vec();
        let verification: Result<()> = async {
            self.operator
                .write_options(
                    &path,
                    initial.clone(),
                    WriteOptions {
                        if_not_exists: true,
                        ..Default::default()
                    },
                )
                .await?;
            let duplicate_create = self
                .operator
                .write_options(
                    &path,
                    initial.clone(),
                    WriteOptions {
                        if_not_exists: true,
                        ..Default::default()
                    },
                )
                .await
                .expect_err("conditional create probe must reject an existing object");
            if duplicate_create.kind() != ErrorKind::ConditionNotMatch {
                return Err(duplicate_create.into());
            }
            let first_etag = self
                .operator
                .stat(&path)
                .await?
                .etag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned)
                .context("shared Catalog probe write did not return an ETag")?;
            let observed = self
                .operator
                .read_options(
                    &path,
                    ReadOptions {
                        if_match: Some(first_etag.clone()),
                        ..Default::default()
                    },
                )
                .await?
                .to_vec();
            if observed != initial {
                return Err(anyhow!(
                    "shared Catalog probe read returned different bytes"
                ));
            }
            let replaced = b"{\"format_version\":1,\"stage\":\"replaced\"}".to_vec();
            self.operator
                .write_options(
                    &path,
                    replaced.clone(),
                    WriteOptions {
                        if_match: Some(first_etag.clone()),
                        ..Default::default()
                    },
                )
                .await?;
            let second_etag = self
                .operator
                .stat(&path)
                .await?
                .etag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned)
                .context("shared Catalog probe replacement did not return an ETag")?;
            if second_etag == first_etag {
                return Err(anyhow!(
                    "shared Catalog probe replacement did not change the ETag"
                ));
            }
            let stale_read = self
                .operator
                .read_options(
                    &path,
                    ReadOptions {
                        if_match: Some(first_etag.clone()),
                        ..Default::default()
                    },
                )
                .await
                .expect_err("conditional read probe must reject a stale ETag");
            if stale_read.kind() != ErrorKind::ConditionNotMatch {
                return Err(stale_read.into());
            }
            let stale_replace = self
                .operator
                .write_options(
                    &path,
                    b"{\"format_version\":1,\"stage\":\"stale\"}".to_vec(),
                    WriteOptions {
                        if_match: Some(first_etag),
                        ..Default::default()
                    },
                )
                .await
                .expect_err("conditional replacement probe must reject a stale ETag");
            if stale_replace.kind() != ErrorKind::ConditionNotMatch {
                return Err(stale_replace.into());
            }
            let observed = self
                .operator
                .read_options(
                    &path,
                    ReadOptions {
                        if_match: Some(second_etag),
                        ..Default::default()
                    },
                )
                .await?
                .to_vec();
            if observed != replaced {
                return Err(anyhow!(
                    "shared Catalog probe replacement returned different bytes"
                ));
            }
            Ok(())
        }
        .await;
        // Probe objects are never coordination state. Always attempt cleanup,
        // including when a capability check fails halfway through; otherwise
        // repeated startup verification leaks one object per attempt.
        let cleanup = match self.operator.delete(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(anyhow!(error)),
        };
        if let Err(error) = verification {
            if let Err(cleanup_error) = cleanup {
                return Err(error.context(format!(
                    "shared Catalog probe cleanup also failed: {cleanup_error:#}"
                )));
            }
            return Err(error);
        }
        cleanup.context("remove shared Catalog verification probe")?;
        if let Some(cache_key) = cache_key {
            SHARED_WRITE_VERIFICATIONS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .expect("shared-write verification cache poisoned")
                .push((cache_key, self.operator.clone()));
        }
        self.write_mode = CatalogWriteMode::Shared;
        Ok(self)
    }

    pub fn iceberg_storage(&self) -> &IcebergStorageConfig {
        &self.storage
    }

    /// The authoritative operator is shared only with the physical Iceberg
    /// adapter so its test-only memory service sees the same immutable table
    /// metadata as Catalog Head operations. Core never receives this handle.
    pub fn iceberg_operator(&self) -> Operator {
        self.operator.clone()
    }

    pub fn warehouse_uri(&self) -> String {
        if self.space_root.is_empty() {
            format!("{}/forms", self.storage.warehouse_uri)
        } else {
            format!("{}/{}/forms", self.storage.warehouse_uri, self.space_root)
        }
    }

    pub fn head_path(&self) -> String {
        self.catalog_path("head.json")
    }

    pub fn publication_path(&self, generation: u64, command_id: &str) -> String {
        self.catalog_path(&format!("publications/{generation}-{command_id}.json"))
    }

    pub fn command_receipt_path(&self, command_id: &str) -> String {
        self.catalog_path(&format!("command-receipts/{command_id}.json"))
    }

    /// Durable named checkpoints are immutable Space objects. They are not
    /// Catalog authority and never participate in Head publication.
    pub fn checkpoint_path(&self, name: &str) -> String {
        self.space_path(&format!("_ugoite/checkpoints/{name}.json"))
    }

    pub async fn read_exact_head(&self) -> Result<Option<ExactCatalogHead>> {
        let Some((bytes, etag)) = self.read_exact_object(&self.head_path()).await? else {
            return Ok(None);
        };
        Ok(Some(ExactCatalogHead { bytes, etag }))
    }

    async fn read_exact_object(&self, path: &str) -> Result<Option<(Vec<u8>, Option<String>)>> {
        if let Some(counter) = &self.read_counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        for attempt in 0..EXACT_HEAD_READ_ATTEMPTS {
            let metadata = match self.operator.stat(path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let etag = metadata
                .etag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned);
            let read = match etag.as_deref() {
                Some(etag) => self
                    .operator
                    .read_options(
                        path,
                        ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .map(|bytes| bytes.to_vec()),
                None if self.write_mode == CatalogWriteMode::Shared => {
                    return Err(anyhow!("exact Catalog object stat did not return an ETag"));
                }
                None => self.operator.read(path).await.map(|bytes| bytes.to_vec()),
            };
            match read {
                Ok(bytes) => return Ok(Some((bytes, etag))),
                Err(error)
                    if error.kind() == ErrorKind::ConditionNotMatch
                        && attempt + 1 < EXACT_HEAD_READ_ATTEMPTS =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact Head read attempts always return or continue")
    }

    async fn replace_exact_object(
        &self,
        path: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        path,
                        bytes,
                        WriteOptions {
                            if_match: Some(
                                etag.context("shared exact object replacement requires an ETag")?
                                    .to_string(),
                            ),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                self.operator.write(path, bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn create_head(&self, bytes: Vec<u8>) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_not_exists: true,
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                if self.operator.exists(&self.head_path()).await? {
                    return Err(anyhow!("Catalog Head already exists"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn replace_head(&self, etag: Option<&str>, bytes: Vec<u8>) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_match: Some(
                                etag.context("shared Catalog Head replacement requires an ETag")?
                                    .to_string(),
                            ),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    /// Creates the deterministic lifecycle marker for one Asset.  The marker
    /// is deliberately outside Catalog Head: Head size must not grow with the
    /// number of deleted blobs.  `if_not_exists` also makes two deletion
    /// attempts contend at the storage boundary.
    pub async fn create_asset_lifecycle_marker(
        &self,
        asset_id: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let path = self.catalog_path(&format!("asset-lifecycle/{asset_id}"));
        self.operator
            .write_options(
                &path,
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_asset_lifecycle_marker(
        &self,
        asset_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
        self.read_exact_object(&self.catalog_path(&format!("asset-lifecycle/{asset_id}")))
            .await
    }

    pub async fn replace_asset_lifecycle_marker(
        &self,
        asset_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.replace_exact_object(
            &self.catalog_path(&format!("asset-lifecycle/{asset_id}")),
            etag,
            bytes,
        )
        .await
    }

    pub async fn delete_asset_lifecycle_marker(&self, asset_id: &str) -> Result<()> {
        let path = self.catalog_path(&format!("asset-lifecycle/{asset_id}"));
        match self.operator.delete(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn create_publication(&self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.operator
            .write_options(
                path,
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_publication(&self, path: &str) -> opendal::Result<Vec<u8>> {
        if let Some(counter) = &self.read_counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        Ok(self.operator.read(path).await?.to_vec())
    }

    pub async fn create_command_receipt(&self, command_id: &str, bytes: Vec<u8>) -> Result<()> {
        self.operator
            .write_options(
                &self.command_receipt_path(command_id),
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_command_receipt(
        &self,
        command_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
        self.read_exact_object(&self.command_receipt_path(command_id))
            .await
    }

    pub async fn replace_command_receipt(
        &self,
        command_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.replace_exact_object(&self.command_receipt_path(command_id), etag, bytes)
            .await
    }

    pub async fn create_checkpoint(&self, name: &str, bytes: Vec<u8>) -> Result<()> {
        if !self
            .operator
            .info()
            .full_capability()
            .write_with_if_not_exists
        {
            return Err(anyhow!(
                "immutable checkpoint creation requires OpenDAL if_not_exists support"
            ));
        }
        self.operator
            .write_options(
                &self.checkpoint_path(name),
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_checkpoint(&self, name: &str) -> opendal::Result<Vec<u8>> {
        Ok(self
            .operator
            .read(&self.checkpoint_path(name))
            .await?
            .to_vec())
    }

    pub fn supports_shared_writes(&self) -> bool {
        let capabilities = self.operator.info().full_capability();
        capabilities.read_with_if_match
            && capabilities.write_with_if_match
            && capabilities.write_with_if_not_exists
    }

    fn shared_write_verification_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:accessor={:p}",
            self.operator.info().scheme(),
            self.operator.info().name(),
            self.operator.info().root(),
            self.space_root,
            Arc::as_ptr(self.operator.inner()),
        )
    }

    pub fn backend_capabilities(&self) -> CatalogBackendCapabilities {
        let capabilities = self.operator.info().full_capability();
        let shared_write_contract = self.supports_shared_writes();
        CatalogBackendCapabilities {
            // OpenDAL exposes ETags through stat metadata rather than a
            // separate capability bit. Exact shared Head reads additionally
            // require the conditional-read contract.
            etag: capabilities.stat && capabilities.read_with_if_match,
            read_with_if_match: capabilities.read_with_if_match,
            write_with_if_match: capabilities.write_with_if_match,
            write_with_if_not_exists: capabilities.write_with_if_not_exists,
            shared_write_contract,
        }
    }

    fn catalog_path(&self, suffix: &str) -> String {
        self.space_path(&format!("_ugoite/catalog/{suffix}"))
    }

    fn space_path(&self, suffix: &str) -> String {
        if self.space_root.is_empty() {
            suffix.to_string()
        } else {
            format!("{}/{suffix}", self.space_root)
        }
    }
}

static CATALOG_SERIALIZERS: OnceLock<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>> =
    OnceLock::new();
static SHARED_WRITE_VERIFICATIONS: OnceLock<Mutex<Vec<(String, Operator)>>> = OnceLock::new();

fn catalog_serializer(operator: &Operator, space_root: &str) -> Arc<AsyncMutex<()>> {
    let key = format!(
        "{}:{}:{}",
        operator.info().scheme(),
        operator.info().root(),
        space_root
    );
    let serializers = CATALOG_SERIALIZERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut serializers = serializers
        .lock()
        .expect("catalog serializer registry poisoned");
    if let Some(serializer) = serializers.get(&key).and_then(Weak::upgrade) {
        return serializer;
    }
    let serializer = Arc::new(AsyncMutex::new(()));
    serializers.insert(key, Arc::downgrade(&serializer));
    serializer
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEntry {
    pub name: String,
    pub is_dir: bool,
}

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn exists(&self, path: &str) -> Result<bool>;
    async fn read(&self, path: &str) -> Result<Vec<u8>>;
    async fn write(&self, path: &str, data: Vec<u8>) -> Result<()>;
    async fn write_if_absent(&self, path: &str, data: Vec<u8>) -> Result<()>;
    async fn set_private(&self, _path: &str) -> Result<()> {
        Ok(())
    }
    async fn create_dir(&self, path: &str) -> Result<()>;
    async fn list_dir(&self, path: &str) -> Result<Vec<StorageEntry>>;

    async fn read_json<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned + Send,
    {
        Ok(serde_json::from_slice(&self.read(path).await?)?)
    }

    async fn write_json<T>(&self, path: &str, value: &T) -> Result<()>
    where
        T: Serialize + Sync,
    {
        self.write(path, serde_json::to_vec_pretty(value)?).await
    }
}

static MEMORY_OPERATORS: OnceLock<Mutex<HashMap<String, Operator>>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<HashMap<String, Operator>> {
    MEMORY_OPERATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn local_operator_from_uri(uri: &str) -> Result<Operator> {
    let root = uri
        .strip_prefix("fs://")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
    let atomic_write_dir = Path::new(root).join(".ugoite-atomic-writes");
    let op = Operator::new(
        Fs::default()
            .root(root)
            .atomic_write_dir(atomic_write_dir.to_string_lossy().as_ref()),
    )?
    .finish();
    Ok(op)
}

pub fn operator_from_uri(uri: &str) -> Result<Operator> {
    operator_from_uri_with_endpoint(uri, None)
}

pub fn operator_from_uri_with_endpoint(uri: &str, endpoint: Option<&str>) -> Result<Operator> {
    if uri.starts_with("memory://") {
        let mut cache = memory_cache()
            .lock()
            .map_err(|_| anyhow::anyhow!("memory operator cache lock poisoned"))?;
        if let Some(op) = cache.get(uri) {
            return Ok(op.clone());
        }
        let op = Operator::new(Memory::default())?.finish();
        cache.insert(uri.to_string(), op.clone());
        return Ok(op);
    }

    if uri.starts_with("fs://")
        || uri.starts_with("file://")
        || uri.starts_with('/')
        || uri.starts_with('.')
    {
        return local_operator_from_uri(uri);
    }

    if uri.starts_with("s3://") {
        let parsed = url::Url::parse(uri)?;
        let bucket = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("s3 storage URI must include a bucket"))?;
        let root = parsed.path().trim_start_matches('/');
        let mut builder = S3::default().bucket(bucket).root(root).region("us-east-1");
        if let Some(endpoint) = endpoint {
            builder = builder.endpoint(endpoint);
        }
        return Ok(Operator::new(builder)?.finish());
    }

    Ok(Operator::from_uri(uri)?)
}

#[derive(Clone)]
pub struct OpendalStorage {
    operator: Operator,
}

impl OpendalStorage {
    pub fn new(operator: Operator) -> Self {
        Self { operator }
    }

    pub fn from_operator(operator: &Operator) -> Self {
        Self::new(operator.clone())
    }

    fn local_path(&self, path: &str) -> Option<std::path::PathBuf> {
        match self.operator.info().scheme() {
            "fs" | "file" => Some(Path::new(self.operator.info().root().as_str()).join(path)),
            _ => None,
        }
    }
}

#[async_trait]
impl StorageBackend for OpendalStorage {
    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.operator.exists(path).await?)
    }

    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        Ok(self.operator.read(path).await?.to_vec())
    }

    async fn write(&self, path: &str, data: Vec<u8>) -> Result<()> {
        self.operator.write(path, data).await?;
        Ok(())
    }

    async fn write_if_absent(&self, path: &str, data: Vec<u8>) -> Result<()> {
        if let Some(target) = self.local_path(path) {
            let parent = target
                .parent()
                .ok_or_else(|| anyhow!("storage path has no parent: {path}"))?;
            let file_name = target
                .file_name()
                .ok_or_else(|| anyhow!("storage path has no file name: {path}"))?
                .to_string_lossy();
            let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).await?;
            file.write_all(&data).await?;
            file.sync_all().await?;

            // A direct create_new(target) makes a partially written target
            // visible to readers. Publish a fully written, synced temporary
            // file through a hard link instead: link(2) is atomic and fails
            // when another writer already owns the destination.
            drop(file);
            let link_result = tokio::fs::hard_link(&temporary, &target).await;
            let cleanup_result = tokio::fs::remove_file(&temporary).await;
            if let Err(error) = link_result {
                let _ = cleanup_result;
                return Err(error.into());
            }
            cleanup_result?;

            #[cfg(unix)]
            if let Ok(directory) = tokio::fs::File::open(parent).await {
                directory.sync_all().await?;
            }

            return Ok(());
        }

        if !self
            .operator
            .info()
            .full_capability()
            .write_with_if_not_exists
        {
            return Err(anyhow!(
                "storage backend does not support conditional object creation"
            ));
        }
        self.operator
            .write_options(
                path,
                data,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn set_private(&self, path: &str) -> Result<()> {
        let Some(target) = self.local_path(path) else {
            return Ok(());
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let root_path = self.operator.info().root();
            let root = Path::new(root_path.as_str());
            let mut current = target.parent();
            while let Some(directory) = current {
                if directory == root {
                    break;
                }
                if tokio::fs::try_exists(directory).await? {
                    tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                        .await?;
                }
                current = directory.parent();
            }

            if tokio::fs::try_exists(&target).await? {
                tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).await?;
            }
        }

        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.operator.create_dir(path).await?;
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<StorageEntry>> {
        let mut entries = Vec::new();
        let mut lister = self.operator.lister(path).await?;
        while let Some(entry) = lister.try_next().await? {
            entries.push(StorageEntry {
                name: entry.name().to_string(),
                is_dir: entry.metadata().mode() == EntryMode::DIR,
            });
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_head_bytes, operator_from_uri, operator_from_uri_with_endpoint,
        DerivedRelationHead, DerivedRelationHeadStore, OpendalStorage, SpaceCatalogStore,
        StorageBackend,
    };
    use anyhow::Result;
    use futures::future::join_all;
    use opendal::services::Memory;
    use opendal::Operator;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn operator_from_uri_supports_fs_and_memory() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let fs_uri = format!("fs://{}", temp_dir.path().display());
        let fs_operator = operator_from_uri(&fs_uri)?;
        fs_operator
            .write("hello.txt", b"hello world".to_vec())
            .await?;
        let fs_bytes = fs_operator.read("hello.txt").await?.to_vec();
        assert_eq!(fs_bytes, b"hello world");

        let memory_operator = operator_from_uri("memory://storage-crate")?;
        memory_operator
            .write("hello.txt", b"hello world".to_vec())
            .await?;
        let memory_bytes = memory_operator.read("hello.txt").await?.to_vec();
        assert_eq!(memory_bytes, b"hello world");

        Ok(())
    }

    #[tokio::test]
    /// REQ-STO-001: JSON helpers remain part of the storage boundary after
    /// I/O is moved out of the portable domain crate.
    async fn test_storage_req_sto_001_json_helpers_use_storage_abstraction() -> Result<()> {
        let op = operator_from_uri("memory://storage-contract")?;
        let storage = OpendalStorage::from_operator(&op);
        storage.create_dir("spaces/demo/").await?;
        storage
            .write("spaces/demo/readme.md", b"hello".to_vec())
            .await?;

        assert!(storage.exists("spaces/demo/readme.md").await?);
        assert!(storage.exists("spaces/demo/").await?);
        assert!(!storage.exists("spaces/missing/").await?);
        assert_eq!(storage.read("spaces/demo/readme.md").await?, b"hello");
        let entries = storage.list_dir("spaces/demo/").await?;
        assert!(entries
            .iter()
            .any(|entry| entry.name.ends_with("readme.md") && !entry.is_dir));

        storage
            .write_json("spaces/demo/meta.json", &serde_json::json!({"id": "demo"}))
            .await?;
        let metadata: serde_json::Value = storage.read_json("spaces/demo/meta.json").await?;
        assert_eq!(metadata["id"], "demo");
        assert_eq!(
            storage.list_dir("spaces/").await?,
            vec![super::StorageEntry {
                name: "demo/".to_string(),
                is_dir: true,
            }]
        );

        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_conditional_create_is_private_and_single_winner() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir()?;
        let op = operator_from_uri(&format!("fs://{}", temp_dir.path().display()))?;
        let storage = OpendalStorage::from_operator(&op);
        storage.create_dir("response_hmac/").await?;

        let results = join_all((0..8).map(|index| {
            let storage = storage.clone();
            async move {
                storage
                    .write_if_absent(
                        "response_hmac/default.json",
                        format!("key-{index}").into_bytes(),
                    )
                    .await
            }
        }))
        .await;

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let published = storage.read("response_hmac/default.json").await?;
        assert!(published.starts_with(b"key-"));
        storage.set_private("response_hmac/default.json").await?;
        let file_mode = std::fs::metadata(temp_dir.path().join("response_hmac/default.json"))?
            .permissions()
            .mode()
            & 0o777;
        let dir_mode = std::fs::metadata(temp_dir.path().join("response_hmac"))?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);

        Ok(())
    }

    #[test]
    fn operator_from_uri_with_endpoint_builds_s3_with_custom_endpoint() -> Result<()> {
        let op = operator_from_uri_with_endpoint(
            "s3://bucket-name/prefix",
            Some("https://s3.example.test"),
        )?;

        assert_eq!(op.info().scheme(), "s3");
        assert_eq!(op.info().root(), "/prefix/");

        Ok(())
    }

    #[tokio::test]
    async fn catalog_head_reads_are_exact_in_single_process_mode() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let store = SpaceCatalogStore::new(operator, "spaces/demo")?;

        assert!(store.read_exact_head().await?.is_none());
        store.create_head(b"first".to_vec()).await?;
        let first = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(first.bytes, b"first");

        store.replace_head(None, b"second".to_vec()).await?;
        let second = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(second.bytes, b"second");

        Ok(())
    }

    #[tokio::test]
    async fn shared_catalog_mode_fails_closed_without_an_exact_etag_contract() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let error = SpaceCatalogStore::new(operator, "spaces/demo")?
            .verify_shared_writes()
            .await
            .expect_err("Memory has no ETag-bound shared-write contract");

        assert!(error
            .to_string()
            .contains("ETag-bound reads and conditional writes"));
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_uses_checksum_and_single_process_publication() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA001);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id)
            .single_process();
        let first = DerivedRelationHead {
            format_version: 1,
            space_id: "demo".into(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            build_id: "build-a".into(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: "table-uuid".into(),
            metadata_location: "memory:///metadata.json".into(),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input-a".into(),
            source_coordinate: serde_json::json!({"catalog_head_sha256":null}),
            checksum: String::new(),
        };
        store.create(&first).await?;
        let exact_first = store.read_exact().await?.expect("derived Head");
        assert!(!exact_first.head.checksum.is_empty());

        let mut second = first.clone();
        second.generation = 2;
        second.build_id = "build-b".into();
        store.replace(None, &second).await?;
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&canonical_head_bytes(&second)?)?;
        invalid["generation"] = serde_json::json!(99);
        operator
            .write(&store.head_path(), serde_json::to_vec(&invalid)?)
            .await?;
        let corrupt = store
            .read_exact()
            .await
            .expect_err("corrupt Relation Head checksum must be rejected");
        assert!(corrupt.to_string().contains("checksum"), "{corrupt}");
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_shared_mode_fails_closed_without_contract() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let operator = operator_from_uri(&format!("fs://{}", temp_dir.path().display()))?;
        let error =
            DerivedRelationHeadStore::new(operator, "spaces/demo", uuid::Uuid::from_u128(0xA001))
                .shared()
                .await
                .expect_err("filesystem backend has no exact shared-write contract");
        assert!(error
            .to_string()
            .contains("ETag-bound reads and conditional writes"));
        Ok(())
    }

    #[tokio::test]
    async fn legacy_derived_head_is_explicitly_invalidated_for_rebuild() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA006);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let legacy_data_path = format!(
            "{}/data/old.parquet",
            store.legacy_materializations_prefix().trim_end_matches('/')
        );
        operator
            .write(&legacy_data_path, b"legacy".to_vec())
            .await?;
        operator
            .write(
                &store.head_path(),
                serde_json::to_vec(&serde_json::json!({
                    "format_version": 1,
                    "space_id": "demo",
                    "relation_id": relation_id,
                    "generation": 1,
                    "definition_version": 1,
                    "definition_fingerprint": "old",
                    "producer_id": "old",
                    "producer_fingerprint": "old",
                    "compatibility_epoch": 1,
                    "materialization_id": "old-materialization",
                    "table_identifier": {"table":"derived"},
                    "table_uuid": "old-table",
                    "metadata_location": "memory:///old",
                    "snapshot_id": null,
                    "schema_id": 0,
                    "base_generation": 0,
                    "target_generation": 1,
                    "build_id": "old-build",
                    "input_digest": "old-input",
                    "source_coordinate": {},
                    "materialization_manifest_location": "old/manifest.json",
                    "materialization_manifest_checksum": "old",
                    "last_command_id": "index:asset-text:1",
                    "checksum": "old"
                }))?,
            )
            .await?;
        let error = store
            .read_exact()
            .await
            .expect_err("legacy Head must not be treated as a current build");
        assert!(error
            .downcast_ref::<super::LegacyDerivedRelationHead>()
            .is_some());
        store.invalidate_legacy_head().await?;
        assert!(!operator.exists(&store.head_path()).await?);
        assert!(!operator.exists(&legacy_data_path).await?);
        Ok(())
    }

    #[tokio::test]
    async fn garbage_age_starts_when_build_is_marked_garbage() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA007);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let path = format!("{}/manifest.json", store.builds_path("stale"));
        operator.write(&path, b"stale".to_vec()).await?;
        assert!(store
            .garbage_collect(None, Duration::from_secs(3600))
            .await?
            .is_empty());
        assert!(operator.exists(&path).await?);
        store.mark_garbage("stale").await?;
        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec!["stale"]);
        assert!(!operator.exists(&path).await?);
        Ok(())
    }

    #[tokio::test]
    async fn garbage_marked_partial_staging_build_is_garbage_collectable() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA008);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let partial = format!("{}/data/partial.parquet", store.builds_path("partial"));
        store.mark_staging("partial").await?;
        operator.write(&partial, b"partial".to_vec()).await?;
        store.mark_garbage("partial").await?;
        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec!["partial"]);
        assert!(!operator.exists(&partial).await?);
        Ok(())
    }

    #[tokio::test]
    async fn garbage_collection_retains_terminal_claim_after_marker_cleanup() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA00C);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let data = format!("{}/data/old.parquet", store.builds_path("old"));
        operator.write(&data, b"old".to_vec()).await?;
        store.mark_garbage("old").await?;

        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec!["old"]
        );
        assert!(!operator.exists(&data).await?);
        assert!(!operator.exists(&store.garbage_marker_path("old")).await?);
        // The terminal garbage claim fences a publisher that was paused
        // before the GC claim was created, even after garbage.json is gone.
        assert!(
            operator
                .exists(&store.publishing_marker_path("old"))
                .await?
        );
        assert!(store
            .begin_publishing("old")
            .await
            .expect_err("a reclaimed build must stay fenced")
            .to_string()
            .contains("claim is held"));
        Ok(())
    }

    #[tokio::test]
    async fn stale_staging_build_gets_durable_cleanup_intent_and_is_collectable() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA009);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let partial = format!("{}/data/crashed.parquet", store.builds_path("crashed"));
        store.mark_staging("crashed").await?;
        operator.write(&partial, b"crashed".to_vec()).await?;

        assert!(store
            .garbage_collect(None, Duration::ZERO)
            .await?
            .is_empty());
        assert!(operator.exists(&partial).await?);
        assert!(
            operator
                .exists(&store.garbage_marker_path("crashed"))
                .await?
        );
        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec!["crashed"]);
        assert!(!operator.exists(&partial).await?);
        assert!(
            !operator
                .exists(&format!("{}/garbage.json", store.builds_path("crashed")))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn fresh_garbage_marker_preserves_staging_grace_period() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA00B);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let data = format!("{}/data/crashed.parquet", store.builds_path("crashed"));
        store.mark_staging("crashed").await?;
        operator.write(&data, b"crashed".to_vec()).await?;
        store.mark_garbage("crashed").await?;

        assert!(store
            .garbage_collect(None, Duration::from_secs(3600))
            .await?
            .is_empty());
        assert!(operator.exists(&data).await?);
        assert!(
            operator
                .exists(&store.garbage_marker_path("crashed"))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn garbage_marker_age_uses_persisted_timestamp_on_memory_backend() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA00D);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let data = format!("{}/data/old.parquet", store.builds_path("old"));
        operator.write(&data, b"old".to_vec()).await?;
        let marked_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .saturating_sub(3600);
        operator
            .write(
                &store.garbage_marker_path("old"),
                serde_json::to_vec(&serde_json::json!({ "marked_at": marked_at }))?,
            )
            .await?;

        assert_eq!(
            store
                .garbage_collect(None, Duration::from_secs(1800))
                .await?,
            vec!["old"]
        );
        assert!(!operator.exists(&data).await?);
        Ok(())
    }

    #[tokio::test]
    async fn markerless_old_orphan_build_is_collectable() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA00A);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let orphan = format!("{}/metadata/orphan.json", store.builds_path("orphan"));
        operator.write(&orphan, b"orphan".to_vec()).await?;

        assert!(store
            .garbage_collect(None, Duration::from_secs(3600))
            .await?
            .is_empty());
        assert!(operator.exists(&orphan).await?);

        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec!["orphan"]);
        assert!(!operator.exists(&orphan).await?);
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_single_process_create_has_one_winner() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA002);
        let store =
            DerivedRelationHeadStore::new(operator, "spaces/demo", relation_id).single_process();
        let head = DerivedRelationHead {
            format_version: 1,
            space_id: "demo".into(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            build_id: "build".into(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: "table-uuid".into(),
            metadata_location: "memory:///metadata.json".into(),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            checksum: String::new(),
        };
        let results = join_all((0..8).map(|_| {
            let store = store.clone();
            let head = head.clone();
            async move { store.create(&head).await }
        }))
        .await;
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(store.read_exact().await?.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn derived_build_gc_never_removes_current_build() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA003);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        operator
            .write(
                &format!("{}/manifest.json", store.builds_path("current")),
                b"current".to_vec(),
            )
            .await?;
        operator
            .write(
                &format!("{}/manifest.json", store.builds_path("stale")),
                b"stale".to_vec(),
            )
            .await?;
        store.mark_garbage("stale").await?;
        let deleted = store
            .garbage_collect(Some("current"), Duration::ZERO)
            .await?;
        assert_eq!(deleted, vec!["stale"]);
        assert!(
            operator
                .exists(&format!("{}/manifest.json", store.builds_path("current")))
                .await?
        );
        assert!(
            !operator
                .exists(&format!("{}/manifest.json", store.builds_path("stale")))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn derived_publish_rejects_a_stale_single_process_writer() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA004);
        let store = DerivedRelationHeadStore::new(operator, "spaces/demo", relation_id);
        let mut first = DerivedRelationHead {
            format_version: 1,
            space_id: "demo".into(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer".into(),
            compatibility_epoch: 1,
            build_id: "build-1".into(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: "table".into(),
            metadata_location: "memory:///metadata".into(),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            checksum: String::new(),
        };
        store.publish(None, &first).await?;
        let expected = store.read_exact().await?.expect("initial Head");
        first.generation = 2;
        first.build_id = "build-2".into();
        store.publish(Some(&expected), &first).await?;
        let mut loser = first.clone();
        loser.generation = 3;
        loser.build_id = "build-3".into();
        let error = store
            .publish(Some(&expected), &loser)
            .await
            .expect_err("stale writer must lose CAS");
        assert!(error.to_string().contains("changed"));
        assert_eq!(store.read_exact().await?.unwrap().head.build_id, "build-2");
        Ok(())
    }

    #[tokio::test]
    async fn single_process_relation_lock_serializes_full_rebuilds() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let store =
            DerivedRelationHeadStore::new(operator, "spaces/demo", uuid::Uuid::from_u128(0xA005));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let results = join_all((0..8).map(|_| {
            let lock = store.single_process_lock();
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            async move {
                let _guard = lock.lock().await;
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(1)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }
        }))
        .await;
        assert_eq!(results.len(), 8);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
