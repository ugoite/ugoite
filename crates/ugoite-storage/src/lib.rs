//! Persistence adapter boundary for Ugoite.

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use futures::TryStreamExt;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::services::{Fs, Memory, S3};
use opendal::{EntryMode, ErrorKind, Operator};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Raw current Head evidence used only to replace a corrupt Derived Head under
/// the same CAS boundary as a normal publication. It is intentionally not a
/// readable relation coordinate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDerivedRelationHead {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogWriteMode {
    Shared,
    SingleProcess,
}

/// The storage-side admission check for authoritative Catalog writes.
///
/// The Iceberg crate performs the product-level admission check as well, but
/// this boundary must fail closed when a caller reaches the raw Catalog store
/// directly.  v1 only permits authoritative writes on local backends; shared
/// object-store mutation requires the higher-level atomic fencing contract.
#[derive(Debug, Clone)]
pub struct CatalogMutationPermit {
    store_key: String,
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
    /// Opaque CAS fence used by shared-backend GC.  GC rotates this value on
    /// the current Head before reclaiming a non-current build, so a publisher
    /// holding the previous Head ETag cannot swap that build back into view.
    #[serde(default)]
    pub head_fence: String,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactDerivedRelationHead {
    pub head: DerivedRelationHead,
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

/// Raw exact coordinate for the pre-current-build DerivedRelation Head.
///
/// This is not a compatibility representation.  It exists only so a shared
/// writer can replace disposable v1 derived state with a new build through the
/// same ETag-bound CAS used for every current Head publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactLegacyDerivedRelationHead {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GarbageHeadFence {
    CandidateIsCurrent,
    Contended,
    /// A legacy v1 Head is still authoritative for the old disposable
    /// materialization layout. It fences only that old coordinate; unrelated
    /// current-build prefixes can still be reclaimed.
    LegacyHead,
    Fenced {
        empty_head: bool,
        etag: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct GarbageClaim {
    etag: Option<String>,
    owner: String,
}

const DERIVED_BUILD_CLAIM_TTL: Duration = Duration::from_secs(15 * 60);
const DERIVED_BUILD_CLAIM_RENEWAL: Duration = Duration::from_secs(30);
const DERIVED_TERMINAL_CLAIM_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_DERIVED_GC_LIST_ENTRIES: usize = 100_000;
const MAX_DERIVED_TERMINAL_TOMBSTONES_PER_PASS: usize = 1_024;
const MAX_ASSET_LIFECYCLE_SCAN_ENTRIES: usize = 100_000;

struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);

impl AbortOnDrop {
    fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self(Some(handle))
    }

    fn abort(mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

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

    fn head_fence_bytes(&self, state: &str, build_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "state": state,
            "relation_id": self.relation_id.to_string(),
            "build_id": build_id,
        }))
        .expect("derived Head fence is serializable")
    }

    fn head_fence(value: &serde_json::Value) -> Option<(&str, &str)> {
        let state = value.get("state").and_then(serde_json::Value::as_str)?;
        let build_id = value.get("build_id").and_then(serde_json::Value::as_str)?;
        match state {
            "garbage_fence" | "head_fence_released" => Some((state, build_id)),
            _ => None,
        }
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

    fn legacy_garbage_marker_path(&self) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/legacy-garbage.json",
            self.space_root, self.relation_id
        )
    }

    fn garbage_marker_path(&self, build_id: &str) -> String {
        format!("{}/garbage.json", self.builds_path(build_id))
    }

    fn terminal_tombstone_path(&self, build_id: &str) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/tombstones/{build_id}.json",
            self.space_root, self.relation_id
        )
    }

    fn terminal_tombstones_prefix(&self) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/tombstones/",
            self.space_root, self.relation_id
        )
    }

    fn staging_marker_path(&self, build_id: &str) -> String {
        format!("{}/staging.json", self.builds_path(build_id))
    }

    /// Create the marker before any immutable build object is written.  A
    /// failed marker write aborts staging before it can leave an unidentifiable
    /// prefix behind.
    pub async fn mark_staging(&self, build_id: &str) -> Result<()> {
        // Build IDs are the durable publication fence.  Reusing an ID after
        // its terminal claim has been reaped would let a paused producer
        // recreate a deleted prefix, so the lifecycle accepts only the UUIDv7
        // IDs generated by the production builder.
        let uuid = Uuid::parse_str(build_id)
            .map_err(|_| anyhow!("DerivedRelation build ID must be UUIDv7"))?;
        if uuid.get_version_num() != 7 || (uuid.as_bytes()[8] & 0xc0) != 0x80 {
            return Err(anyhow!("DerivedRelation build ID must be UUIDv7"));
        }
        // Build IDs are never admitted based on their producer clock. The
        // durable tombstone is the non-reuse fence, including for shared
        // backends whose object timestamps are unavailable.
        if self
            .operator
            .exists(&self.terminal_tombstone_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build ID has a terminal tombstone"));
        }
        if self
            .operator
            .exists(&self.garbage_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build is already garbage"));
        }
        if let Some((bytes, _, _)) = self.read_build_claim(build_id).await? {
            if matches!(
                Self::claim_role(&bytes).as_deref(),
                Some("garbage") | Some("complete") | Some("reaping")
            ) {
                return Err(anyhow!(
                    "DerivedRelation build has a terminal garbage claim"
                ));
            }
        }
        if self.write_mode == CatalogWriteMode::Shared {
            self.operator
                .write_options(
                    &self.publishing_marker_path(build_id),
                    Self::build_claim_bytes(build_id, "staging", build_id),
                    WriteOptions {
                        if_not_exists: true,
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)?;
        }
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

    /// Refresh the staging marker while an immutable build is still running.
    /// The persisted timestamp is part of the lifecycle contract: backends
    /// without object modification metadata still need a durable age boundary
    /// after a process crash.
    pub async fn renew_staging(&self, build_id: &str) -> Result<()> {
        if !self
            .operator
            .exists(&self.staging_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation staging lease disappeared"));
        }
        if self.write_mode == CatalogWriteMode::Shared
            && !self.renew_claim_role(build_id, "staging").await?
        {
            return Err(anyhow!("DerivedRelation staging claim was lost"));
        }
        self.operator
            .write(
                &self.staging_marker_path(build_id),
                Self::build_marker_bytes(),
            )
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    fn publishing_marker_path(&self, build_id: &str) -> String {
        format!("{}/publishing.json", self.builds_path(build_id))
    }

    fn build_claim_bytes(build_id: &str, role: &str, owner: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "build_id": build_id,
            "role": role,
            "owner": owner,
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

    fn claim_is_stale(_bytes: &[u8], last_modified: Option<SystemTime>) -> bool {
        // Claim timestamps are written by an arbitrary producer clock. Only
        // backend metadata is suitable for a shared lease deadline; when it
        // is unavailable, fail closed and let an operator/repair pass decide.
        last_modified
            .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
            .is_some_and(|age| age >= DERIVED_BUILD_CLAIM_TTL)
    }

    fn build_id_time(build_id: &str) -> Option<SystemTime> {
        let uuid = Uuid::parse_str(build_id).ok()?;
        // Only UUIDv7 has a durable timestamp in the first six bytes.  Treat
        // every other identifier as unknown rather than interpreting random
        // UUIDv4 bits as an age and deleting a fresh orphan.
        if uuid.get_version_num() != 7 || (uuid.as_bytes()[8] & 0xc0) != 0x80 {
            return None;
        }
        let bytes = uuid.into_bytes();
        // UUIDv7 stores its creation time as a 48-bit big-endian Unix
        // millisecond timestamp. This helper is retained for single-process
        // orphan recovery only; shared GC never trusts a producer timestamp.
        let millis = u64::from_be_bytes([
            0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ]);
        Some(UNIX_EPOCH + Duration::from_millis(millis))
    }

    fn old_enough(timestamp: Option<SystemTime>, minimum_gc_age: Duration) -> bool {
        Self::old_enough_at(timestamp, minimum_gc_age, SystemTime::now())
    }

    fn old_enough_at(
        timestamp: Option<SystemTime>,
        minimum_gc_age: Duration,
        now: SystemTime,
    ) -> bool {
        minimum_gc_age.is_zero()
            || timestamp
                .and_then(|timestamp| now.duration_since(timestamp).ok())
                .is_some_and(|age| age >= minimum_gc_age)
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
        // Shared GC must fail closed when the backend does not provide a
        // server-side modification time. Producer-written marker timestamps
        // are unsafe under clock skew. Single-process/local backends may use
        // the marker payload as a bounded recovery fallback.
        if metadata_time.is_some() {
            return metadata_time;
        }
        if self.write_mode == CatalogWriteMode::Shared {
            return None;
        }
        self.operator
            .read(path)
            .await
            .ok()
            .and_then(|bytes| Self::marker_time(&bytes.to_vec()))
    }

    async fn marker_old_enough(&self, path: &str, minimum_gc_age: Duration) -> Result<bool> {
        let metadata = match self.operator.stat(path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.into()),
        };
        if minimum_gc_age.is_zero() {
            return Ok(true);
        }
        Ok(self
            .marker_time_or_metadata(path, metadata.last_modified().map(Into::into))
            .await
            .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
            .is_some_and(|age| age >= minimum_gc_age))
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

    fn claim_owner(bytes: &[u8]) -> Option<String> {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("owner")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
    }

    async fn replace_build_claim(
        &self,
        build_id: &str,
        expected_etag: Option<&str>,
        role: &str,
        owner: &str,
    ) -> Result<bool> {
        let path = self.publishing_marker_path(build_id);
        let result = match (self.write_mode, expected_etag) {
            (CatalogWriteMode::Shared, Some(etag)) => {
                self.operator
                    .write_options(
                        &path,
                        Self::build_claim_bytes(build_id, role, owner),
                        WriteOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
            }
            (CatalogWriteMode::SingleProcess, _) => {
                self.operator
                    .write(&path, Self::build_claim_bytes(build_id, role, owner))
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
        // A producer may be paused after GC has removed its prefix and later
        // resume with the same build ID.  Do not recreate publishing.json for
        // a build whose durable staging lease has already disappeared.
        if !self
            .operator
            .exists(&self.staging_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build is no longer staged"));
        }
        let path = self.publishing_marker_path(build_id);
        let result = self
            .operator
            .write_options(
                &path,
                Self::build_claim_bytes(build_id, "publishing", build_id),
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
            match Self::claim_role(&bytes).as_deref() {
                // The builder owns this build's staging claim and is the only
                // actor allowed to transition it to publication.  This
                // transition must not wait for the claim TTL: a healthy build
                // can finish staging while its heartbeat is still fresh.
                Some("staging") => {
                    if !self
                        .replace_build_claim(
                            build_id,
                            etag.as_deref(),
                            "publishing",
                            &Self::claim_owner(&bytes).unwrap_or_else(|| build_id.to_string()),
                        )
                        .await?
                    {
                        return Err(anyhow!("DerivedRelation build claim is held"));
                    }
                }
                Some("publishing") => {
                    if !Self::claim_is_stale(&bytes, last_modified)
                        || !self
                            .replace_build_claim(
                                build_id,
                                etag.as_deref(),
                                "publishing",
                                &Self::claim_owner(&bytes).unwrap_or_else(|| build_id.to_string()),
                            )
                            .await?
                    {
                        return Err(anyhow!("DerivedRelation build claim is held"));
                    }
                }
                Some("released") => {
                    if !self
                        .replace_build_claim(
                            build_id,
                            etag.as_deref(),
                            "publishing",
                            &Self::claim_owner(&bytes).unwrap_or_else(|| build_id.to_string()),
                        )
                        .await?
                    {
                        return Err(anyhow!("DerivedRelation build claim is held"));
                    }
                }
                _ => return Err(anyhow!("DerivedRelation build claim is held")),
            }
        }
        if let Err(error) = self.ensure_build_publishable(build_id).await {
            let cleanup = self.release_publishing_claim(build_id).await;
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup_error) => error.context(format!(
                    "release DerivedRelation publishing claim after preflight failure: {cleanup_error:#}"
                )),
            });
        }
        Ok(())
    }

    /// Claims the same durable marker used by publication before GC writes a
    /// garbage marker or deletes any build object. The if-match replacement
    /// is the shared-backend exclusion primitive: either publication owns the
    /// marker, or GC owns it, never both.
    async fn current_garbage_claim(&self, build_id: &str) -> Result<Option<GarbageClaim>> {
        let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(None);
        };
        if Self::claim_role(&bytes).as_deref() != Some("garbage") {
            return Ok(None);
        }
        let Some(owner) = Self::claim_owner(&bytes) else {
            return Ok(None);
        };
        Ok(Some(GarbageClaim { etag, owner }))
    }

    async fn garbage_claim_for_owner(
        &self,
        build_id: &str,
        owner: &str,
    ) -> Result<Option<GarbageClaim>> {
        let Some(claim) = self.current_garbage_claim(build_id).await? else {
            return Ok(None);
        };
        if claim.owner != owner {
            return Ok(None);
        }
        Ok(Some(claim))
    }

    async fn claim_build_for_garbage(&self, build_id: &str) -> Result<Option<GarbageClaim>> {
        let path = self.publishing_marker_path(build_id);
        let owner = Uuid::now_v7().to_string();
        match self
            .operator
            .write_options(
                &path,
                Self::build_claim_bytes(build_id, "garbage", &owner),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => self.garbage_claim_for_owner(build_id, &owner).await,
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => {
                let Some((bytes, etag, last_modified)) = self.read_build_claim(build_id).await?
                else {
                    return Ok(None);
                };
                if !matches!(
                    Self::claim_role(&bytes).as_deref(),
                    Some("staging")
                        | Some("publishing")
                        | Some("garbage")
                        | Some("released")
                        | Some("complete")
                ) {
                    return Ok(None);
                }
                if Self::claim_role(&bytes).as_deref() == Some("released") {
                    if !self
                        .replace_build_claim(build_id, etag.as_deref(), "garbage", &owner)
                        .await?
                    {
                        return Ok(None);
                    }
                    return self.garbage_claim_for_owner(build_id, &owner).await;
                }
                if self.write_mode == CatalogWriteMode::SingleProcess
                    && Self::claim_role(&bytes).as_deref() == Some("garbage")
                {
                    // The relation-local mutex is the single-process
                    // exclusion primitive. A prior maintenance pass may
                    // have left its own terminal claim while deliberately
                    // waiting for the garbage marker grace boundary.
                    return Ok(Self::claim_build_for_garbage_single_process_claim(
                        etag, &bytes,
                    ));
                }
                if Self::claim_role(&bytes).as_deref() != Some("complete")
                    && !Self::claim_is_stale(&bytes, last_modified)
                {
                    return Ok(None);
                }
                if !self
                    .replace_build_claim(build_id, etag.as_deref(), "garbage", &owner)
                    .await?
                {
                    return Ok(None);
                }
                self.garbage_claim_for_owner(build_id, &owner).await
            }
            Err(error) => Err(error.into()),
        }
    }

    fn claim_build_for_garbage_single_process_claim(
        etag: Option<String>,
        bytes: &[u8],
    ) -> Option<GarbageClaim> {
        Self::claim_owner(bytes).map(|owner| GarbageClaim { etag, owner })
    }

    async fn renew_claim_role(&self, build_id: &str, role: &str) -> Result<bool> {
        let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        if Self::claim_role(&bytes).as_deref() != Some(role) {
            return Ok(false);
        }
        let Some(owner) = Self::claim_owner(&bytes) else {
            return Ok(false);
        };
        self.replace_build_claim(build_id, etag.as_deref(), role, &owner)
            .await
    }

    /// Keep a shared publisher's claim fresh until its final Head CAS returns.
    /// The claim is durable coordination state, so a slow object-store write
    /// must not look like a crashed publisher to GC.
    async fn renew_publishing_claim(&self, build_id: &str) -> Result<bool> {
        self.renew_claim_role(build_id, "publishing").await
    }

    fn start_publishing_claim_heartbeat(&self, build_id: &str) -> (AbortOnDrop, Arc<AtomicBool>) {
        let store = self.clone();
        let build_id = build_id.to_string();
        let lost = Arc::new(AtomicBool::new(false));
        let heartbeat_lost = lost.clone();
        let heartbeat = AbortOnDrop::new(tokio::spawn(async move {
            loop {
                tokio::time::sleep(DERIVED_BUILD_CLAIM_RENEWAL).await;
                match store.renew_publishing_claim(&build_id).await {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        heartbeat_lost.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        }));
        (heartbeat, lost)
    }

    async fn release_publishing_claim(&self, build_id: &str) -> Result<()> {
        let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
            return Err(anyhow!("DerivedRelation publishing claim disappeared"));
        };
        if Self::claim_role(&bytes).as_deref() != Some("publishing") {
            return Err(anyhow!("DerivedRelation publishing claim was lost"));
        }
        let owner =
            Self::claim_owner(&bytes).context("DerivedRelation publishing claim has no owner")?;
        if owner != build_id {
            return Err(anyhow!("DerivedRelation publishing claim owner changed"));
        }
        let released = self
            .replace_build_claim(build_id, etag.as_deref(), "released", &owner)
            .await?;
        if !released {
            return Err(anyhow!(
                "DerivedRelation publishing claim changed before release"
            ));
        }
        Ok(())
    }

    /// Finish every publishing attempt by making one best-effort, owner-
    /// checked transition out of `publishing`. In particular, do not use `?`
    /// on the Head write before releasing the claim: a failed CAS or a lost
    /// heartbeat is still a terminal build outcome, and leaving the claim in
    /// place would make GC wait for its full TTL.
    async fn finish_publishing_claim(
        &self,
        build_id: &str,
        publish_result: Result<()>,
        heartbeat_lost: bool,
    ) -> Result<()> {
        let release_result = self.release_publishing_claim(build_id).await;
        match (publish_result, heartbeat_lost, release_result) {
            (Err(error), _, Ok(())) => Err(error),
            (Err(error), _, Err(release_error)) => Err(error.context(format!(
                "release DerivedRelation publishing claim after publish failure: {release_error:#}"
            ))),
            (Ok(()), true, Ok(())) => Err(anyhow!(
                "DerivedRelation publishing claim heartbeat was lost"
            )),
            (Ok(()), true, Err(release_error)) => Err(anyhow!(
                "DerivedRelation publishing claim heartbeat was lost; release failed: {release_error:#}"
            )),
            (Ok(()), false, release_result) => release_result,
        }
    }

    /// Refresh the garbage claim before each destructive object operation.
    /// This keeps a long-running deletion from becoming an apparently stale
    /// claim and fences publication from a reclaimed build.
    async fn renew_garbage_claim(&self, build_id: &str, claim: &mut GarbageClaim) -> Result<bool> {
        let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        if Self::claim_role(&bytes).as_deref() != Some("garbage") {
            return Ok(false);
        }
        if Self::claim_owner(&bytes).as_deref() != Some(claim.owner.as_str()) {
            return Ok(false);
        }
        if self.write_mode == CatalogWriteMode::Shared && etag.as_deref() != claim.etag.as_deref() {
            return Ok(false);
        }
        let renewed = self
            .replace_build_claim(build_id, etag.as_deref(), "garbage", &claim.owner)
            .await?;
        if renewed {
            let Some((bytes, etag, _)) = self.read_build_claim(build_id).await? else {
                return Ok(false);
            };
            if Self::claim_role(&bytes).as_deref() != Some("garbage")
                || Self::claim_owner(&bytes).as_deref() != Some(claim.owner.as_str())
            {
                return Ok(false);
            }
            claim.etag = etag;
        }
        Ok(renewed)
    }

    /// Fence a shared Head before destructive cleanup claims a build.  The
    /// claim marker and Head are separate objects, so checking the marker
    /// immediately before the final Head CAS would still leave a TOCTOU race:
    /// GC could claim the build after that check.  Rotating this field with an
    /// ETag-bound Head CAS closes the race.  A publisher using the old exact
    /// Head loses the CAS; a publisher that starts afterwards sees the garbage
    /// claim and cannot claim the build.
    async fn fence_head_before_garbage(&self, build_id: &str) -> Result<GarbageHeadFence> {
        if self.write_mode != CatalogWriteMode::Shared {
            // The relation-local single-process mutex already excludes a
            // rebuild from this GC path.
            return Ok(GarbageHeadFence::Fenced {
                empty_head: false,
                etag: None,
            });
        }
        for _ in 0..3 {
            match self.read_raw_exact().await? {
                None => {
                    // A shared first publication uses if-not-exists.  Install
                    // a temporary object at that exact coordinate so a
                    // publisher cannot pass its preflight check and create a
                    // Head while GC is deleting this build.
                    let result = self
                        .operator
                        .write_options(
                            &self.head_path(),
                            self.head_fence_bytes("garbage_fence", build_id),
                            WriteOptions {
                                if_not_exists: true,
                                ..Default::default()
                            },
                        )
                        .await;
                    match result {
                        Ok(_) => {
                            let etag = self
                                .operator
                                .stat(&self.head_path())
                                .await?
                                .etag()
                                .filter(|etag| !etag.is_empty())
                                .map(str::to_owned)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "shared empty DerivedRelation Head fence requires an ETag"
                                    )
                                })?;
                            return Ok(GarbageHeadFence::Fenced {
                                empty_head: true,
                                etag: Some(etag),
                            });
                        }
                        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => continue,
                        Err(error) => return Err(error.into()),
                    }
                }
                Some((value, _, etag)) => {
                    if let Some((state, fenced_build_id)) = Self::head_fence(&value) {
                        if state == "head_fence_released" {
                            // Reuse the released empty-head fence for the next
                            // candidate. A relation can have several garbage
                            // builds; leaving this sentinel in place would
                            // make every later candidate look contended and
                            // strand its prefix forever.
                            let Some(etag) = etag else {
                                return Err(anyhow!(
                                    "shared empty DerivedRelation Head fence requires an ETag"
                                ));
                            };
                            match self
                                .operator
                                .write_options(
                                    &self.head_path(),
                                    self.head_fence_bytes("garbage_fence", build_id),
                                    WriteOptions {
                                        if_match: Some(etag),
                                        ..Default::default()
                                    },
                                )
                                .await
                            {
                                Ok(_) => {
                                    let etag = self
                                        .operator
                                        .stat(&self.head_path())
                                        .await?
                                        .etag()
                                        .filter(|etag| !etag.is_empty())
                                        .map(str::to_owned)
                                        .ok_or_else(|| {
                                            anyhow!(
                                                "shared empty DerivedRelation Head fence requires an ETag"
                                            )
                                        })?;
                                    return Ok(GarbageHeadFence::Fenced {
                                        empty_head: true,
                                        etag: Some(etag),
                                    });
                                }
                                Err(error) if error.kind() == ErrorKind::ConditionNotMatch => {
                                    continue
                                }
                                Err(error) => return Err(error.into()),
                            }
                        }
                        if fenced_build_id == build_id {
                            return Ok(GarbageHeadFence::Fenced {
                                empty_head: true,
                                etag,
                            });
                        }
                        return Ok(GarbageHeadFence::Contended);
                    }

                    let current: DerivedRelationHead = match serde_json::from_value(value.clone()) {
                        Ok(current) => current,
                        Err(_error) if is_legacy_derived_head(&value) => {
                            return Ok(GarbageHeadFence::LegacyHead);
                        }
                        Err(error) => return Err(anyhow!("decode DerivedRelation Head: {error}")),
                    };
                    validate_derived_head_checksum(&current)?;
                    if current.build_id == build_id {
                        return Ok(GarbageHeadFence::CandidateIsCurrent);
                    }
                    let fenced = DerivedRelationHead {
                        head_fence: Uuid::new_v4().to_string(),
                        ..current
                    };
                    let bytes = canonical_head_bytes(&fenced)?;
                    let Some(etag) = etag else {
                        return Err(anyhow!(
                            "shared DerivedRelation Head fence requires an ETag"
                        ));
                    };
                    match self
                        .operator
                        .write_options(
                            &self.head_path(),
                            bytes,
                            WriteOptions {
                                if_match: Some(etag),
                                ..Default::default()
                            },
                        )
                        .await
                    {
                        Ok(_) => {
                            return Ok(GarbageHeadFence::Fenced {
                                empty_head: false,
                                etag: None,
                            })
                        }
                        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => continue,
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        Err(anyhow!(
            "DerivedRelation Head changed while GC was fencing publication"
        ))
    }

    async fn release_empty_head_fence(&self, build_id: &str, expected_etag: &str) -> Result<bool> {
        let Some((value, _, etag)) = self.read_raw_exact().await? else {
            return Ok(false);
        };
        let Some((state, fenced_build_id)) = Self::head_fence(&value) else {
            return Ok(false);
        };
        if fenced_build_id != build_id {
            return Ok(false);
        }
        if state == "head_fence_released" {
            return Ok(true);
        }
        if etag.as_deref() != Some(expected_etag) {
            return Ok(false);
        }
        match self
            .operator
            .write_options(
                &self.head_path(),
                self.head_fence_bytes("head_fence_released", build_id),
                WriteOptions {
                    if_match: Some(expected_etag.to_string()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Recover the only empty-head state that can remain after a successful
    /// marker-last cleanup: the process may have crashed after the terminal
    /// garbage claim was written but before the fence was released.  The
    /// claim role is the durable proof that the build prefix was already
    /// cleaned, so this recovery does not depend on a fresh listing or on the
    /// claim still being eligible for GC.
    async fn recover_completed_empty_head_fence(&self) -> Result<()> {
        if self.write_mode != CatalogWriteMode::Shared {
            return Ok(());
        }
        let Some((value, _, etag)) = self.read_raw_exact().await? else {
            return Ok(());
        };
        let Some((state, build_id)) = Self::head_fence(&value) else {
            return Ok(());
        };
        if state != "garbage_fence" {
            return Ok(());
        }
        let Some((bytes, _, _)) = self.read_build_claim(build_id).await? else {
            return Ok(());
        };
        if Self::claim_role(&bytes).as_deref() != Some("complete") {
            return Ok(());
        }
        let Some(etag) = etag else {
            return Err(anyhow!(
                "shared empty DerivedRelation Head fence requires an ETag"
            ));
        };
        let _ = self.release_empty_head_fence(build_id, &etag).await?;
        Ok(())
    }

    /// Mark a build at the moment it stops being current.  GC uses this
    /// marker's timestamp rather than the build's creation timestamp so an
    /// old, long-lived current build still gets a full reader grace period
    /// after the Head swap.
    pub async fn mark_garbage(&self, build_id: &str) -> Result<()> {
        // GC retains the publishing claim as a terminal tombstone after it
        // has deleted the build. A delayed producer may still finish an
        // in-flight object write after that transition, so a completed claim
        // is allowed to regain a marker only when new build objects exist.
        // This makes late objects discoverable without resurrecting an empty
        // marker forever after a harmless delayed cleanup callback.
        if let Some((bytes, _, _)) = self.read_build_claim(build_id).await? {
            if Self::claim_role(&bytes).as_deref() == Some("complete") {
                let has_objects = self
                    .list_derived_entries(&self.builds_path(build_id))
                    .await?
                    .into_iter()
                    .any(|entry| {
                        entry.metadata().mode() == EntryMode::FILE
                            && entry.path() != self.garbage_marker_path(build_id)
                            && entry.path() != self.staging_marker_path(build_id)
                            && entry.path() != self.publishing_marker_path(build_id)
                    });
                if !has_objects {
                    return Ok(());
                }
            }
        }
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

    /// Release a GC claim only if the claim still belongs to this collector.
    /// OpenDAL has no conditional delete, so the shared path uses an ETag-bound
    /// transition to an explicitly unclaimed role. A later GC or publisher can
    /// take that role over with its own conditional replacement, while a paused
    /// old collector cannot remove the newer owner's claim.
    async fn release_garbage_claim(&self, build_id: &str, claim: &GarbageClaim) -> Result<bool> {
        let Some((bytes, current_etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        if Self::claim_role(&bytes).as_deref() != Some("garbage") {
            return Ok(false);
        }
        if Self::claim_owner(&bytes).as_deref() != Some(claim.owner.as_str()) {
            return Ok(false);
        }
        if self.write_mode == CatalogWriteMode::Shared
            && current_etag.as_deref() != claim.etag.as_deref()
        {
            return Ok(false);
        }
        let expected_etag = if self.write_mode == CatalogWriteMode::Shared {
            claim.etag.as_deref()
        } else {
            current_etag.as_deref()
        };
        self.replace_build_claim(build_id, expected_etag, "released", &claim.owner)
            .await
    }

    /// Turn a successfully cleaned garbage claim into a terminal tombstone.
    /// OpenDAL cannot conditionally delete the claim object, so the explicit
    /// terminal role prevents `has_pending_garbage` from repeatedly waking for
    /// an already-empty build while still fencing delayed publishers.
    async fn complete_garbage_claim(&self, build_id: &str, claim: &GarbageClaim) -> Result<bool> {
        let Some((bytes, current_etag, _)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        if Self::claim_role(&bytes).as_deref() != Some("garbage") {
            return Ok(false);
        }
        if Self::claim_owner(&bytes).as_deref() != Some(claim.owner.as_str()) {
            return Ok(false);
        }
        let expected_etag = if self.write_mode == CatalogWriteMode::Shared {
            claim.etag.as_deref()
        } else {
            current_etag.as_deref()
        };
        self.replace_build_claim(build_id, expected_etag, "complete", &claim.owner)
            .await
    }

    async fn reap_terminal_claim(&self, build_id: &str) -> Result<bool> {
        let Some((bytes, etag, last_modified)) = self.read_build_claim(build_id).await? else {
            return Ok(false);
        };
        let role = Self::claim_role(&bytes);
        if !matches!(role.as_deref(), Some("complete") | Some("reaping")) {
            return Ok(false);
        }
        let owner =
            Self::claim_owner(&bytes).context("terminal DerivedRelation claim has no owner")?;
        if !Self::old_enough(last_modified, DERIVED_TERMINAL_CLAIM_RETENTION) {
            return Ok(false);
        }
        if role.as_deref() == Some("complete")
            && !self
                .replace_build_claim(build_id, etag.as_deref(), "reaping", &owner)
                .await?
        {
            return Ok(false);
        }
        // The tombstone is deliberately outside the disposable build prefix.
        // It is the durable non-reuse fence that lets the claim itself be
        // removed without allowing a paused producer to resurrect the build.
        match self
            .operator
            .write_options(
                &self.terminal_tombstone_path(build_id),
                serde_json::to_vec(&serde_json::json!({
                    "build_id": build_id,
                    "state": "complete",
                }))?,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => {}
            Err(error) => return Err(error.into()),
        }
        // The role transition is the ownership hand-off. No publisher or GC
        // may replace a `reaping` claim, so this idempotent delete cannot erase
        // a newer owner's claim.
        match self
            .operator
            .delete(&self.publishing_marker_path(build_id))
            .await
        {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    async fn reap_expired_terminal_tombstones(&self) -> Result<()> {
        self.reap_expired_terminal_tombstones_at(SystemTime::now())
            .await
    }

    async fn reap_expired_terminal_tombstones_at(&self, now: SystemTime) -> Result<()> {
        self.reap_expired_terminal_tombstones_at_with_retention(
            now,
            DERIVED_TERMINAL_CLAIM_RETENTION,
        )
        .await
    }

    async fn reap_expired_terminal_tombstones_at_with_retention(
        &self,
        now: SystemTime,
        retention: Duration,
    ) -> Result<()> {
        let mut lister = self
            .operator
            .lister_with(&self.terminal_tombstones_prefix())
            .recursive(true)
            .await?;
        let mut examined = 0usize;
        let mut deleted = 0usize;
        while let Some(entry) = lister.try_next().await? {
            examined = examined.saturating_add(1);
            if examined > MAX_DERIVED_GC_LIST_ENTRIES {
                break;
            }
            if entry.metadata().mode() != EntryMode::FILE
                || !Self::old_enough_at(
                    entry.metadata().last_modified().map(Into::into),
                    retention,
                    now,
                )
            {
                continue;
            }
            match self.operator.delete(entry.path()).await {
                Ok(()) => deleted = deleted.saturating_add(1),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if deleted >= MAX_DERIVED_TERMINAL_TOMBSTONES_PER_PASS {
                break;
            }
        }
        Ok(())
    }

    async fn ensure_build_publishable(&self, build_id: &str) -> Result<()> {
        if self
            .operator
            .exists(&self.garbage_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build is marked garbage"));
        }
        if self
            .operator
            .exists(&self.terminal_tombstone_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build has a terminal tombstone"));
        }
        // garbage.json is removed last, but the publishing marker is retained
        // as the terminal tombstone after cleanup.  Check both lifecycle
        // records so a delayed publisher cannot resurrect a build after GC
        // has already claimed and deleted it.
        if let Some((bytes, _, _)) = self.read_build_claim(build_id).await? {
            match Self::claim_role(&bytes).as_deref() {
                Some("publishing") => {}
                Some("garbage") | Some("complete") => {
                    return Err(anyhow!(
                        "DerivedRelation build has a terminal garbage claim"
                    ));
                }
                Some("released") => {}
                _ => return Err(anyhow!("DerivedRelation build claim is held")),
            }
        }
        // The staging marker is the producer's durable lease.  Once GC has
        // removed the build prefix (and therefore this marker), a paused
        // producer may not recreate the publishing claim and resurrect the
        // deleted build after the terminal claim retention window.
        if !self
            .operator
            .exists(&self.staging_marker_path(build_id))
            .await?
        {
            return Err(anyhow!("DerivedRelation build is no longer staged"));
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

    async fn list_derived_entries(&self, prefix: &str) -> Result<Vec<opendal::Entry>> {
        // Consume the backend lister incrementally. `list_with(...).await`
        // materializes an unbounded Vec before the caller can enforce a
        // safety limit, which makes a corrupted or adversarial prefix a
        // memory-exhaustion vector.
        let mut lister = self.operator.lister_with(prefix).recursive(true).await?;
        let mut entries = Vec::new();
        while let Some(entry) = lister.try_next().await? {
            if entries.len() >= MAX_DERIVED_GC_LIST_ENTRIES {
                return Err(anyhow!(
                    "DerivedRelation GC listing exceeds the {}-object safety bound",
                    MAX_DERIVED_GC_LIST_ENTRIES
                ));
            }
            entries.push(entry);
        }
        Ok(entries)
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

    /// Reports whether a non-current build still needs a future maintenance
    /// pass. A terminal `publishing.json` tombstone after marker-last cleanup
    /// is intentional fencing state, but an incomplete garbage claim without
    /// its marker is recoverable cleanup intent and must wake maintenance.
    pub async fn has_pending_garbage(
        &self,
        current_build_id: Option<&str>,
        minimum_gc_age: Duration,
    ) -> Result<bool> {
        let mut tombstones = self
            .operator
            .lister_with(&self.terminal_tombstones_prefix())
            .recursive(true)
            .await?;
        let mut examined_tombstones = 0usize;
        while let Some(entry) = tombstones.try_next().await? {
            examined_tombstones = examined_tombstones.saturating_add(1);
            if examined_tombstones > MAX_DERIVED_GC_LIST_ENTRIES {
                // A bounded maintenance pass must stay scheduled when the
                // tombstone prefix itself is too large to examine completely.
                // The next pass can continue discovery without allowing one
                // malformed prefix to monopolize a worker forever.
                return Ok(true);
            }
            if entry.metadata().mode() == EntryMode::FILE {
                // Keep maintenance scheduled while the bounded tombstone
                // retention window is active or until a later pass removes
                // the expired record.
                return Ok(true);
            }
        }
        let prefix = format!(
            "{}/_ugoite/derived/relations/{}/builds/",
            self.space_root, self.relation_id
        );
        let entries = self.list_derived_entries(&prefix).await?;
        let observed_current_build_id = self.current_build_id().await?;
        // The argument is only a scheduling hint.  Never use it as authority:
        // after Head removal, falling back to a stale hint could hide an
        // orphan forever.
        let _ = current_build_id;
        let current_build_id = observed_current_build_id.as_deref();
        #[derive(Default)]
        struct Candidate {
            has_garbage_marker: bool,
            has_garbage_fence: bool,
            has_staging_marker: bool,
            has_publishing_marker: bool,
            has_build_object: bool,
            stale_staging_old_enough: bool,
            newest_object_modified: Option<SystemTime>,
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
            let marker_time = if is_staging_marker {
                self.marker_time_or_metadata(entry.path(), modified).await
            } else {
                modified
            };
            let old_enough = minimum_gc_age.is_zero()
                || marker_time
                    .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
                    .is_some_and(|age| age >= minimum_gc_age);
            let candidate = candidates.entry(build_id.to_string()).or_default();
            candidate.has_garbage_marker |= is_garbage_marker;
            candidate.has_staging_marker |= is_staging_marker;
            candidate.has_publishing_marker |= is_publishing_marker;
            if is_staging_marker {
                candidate.stale_staging_old_enough |= old_enough;
            }
            if !is_garbage_marker && !is_staging_marker && !is_publishing_marker {
                candidate.has_build_object = true;
                if let Some(modified) = modified {
                    candidate.newest_object_modified = Some(
                        candidate
                            .newest_object_modified
                            .map_or(modified, |current| current.max(modified)),
                    );
                }
            }
        }
        if let Some((value, _, _)) = self.read_raw_exact().await? {
            if let Some((state, build_id)) = Self::head_fence(&value) {
                if state == "garbage_fence" && Some(build_id) != current_build_id {
                    candidates
                        .entry(build_id.to_string())
                        .or_default()
                        .has_garbage_fence = true;
                }
            }
        }
        for (build_id, candidate) in candidates {
            if candidate.has_garbage_fence {
                // A marker-less garbage fence is itself an in-progress
                // cleanup record. Keep the maintenance scheduler awake until
                // the GC pass can finish its marker-last recovery.
                return Ok(true);
            }
            if candidate.has_garbage_marker {
                let complete =
                    self.read_build_claim(&build_id)
                        .await?
                        .is_some_and(|(bytes, _, _)| {
                            matches!(
                                Self::claim_role(&bytes).as_deref(),
                                Some("complete") | Some("reaping")
                            )
                        });
                if complete && !candidate.has_build_object {
                    // A delayed publisher may have recreated the marker after
                    // marker-last cleanup. With no late objects, the terminal
                    // claim is the durable proof that no build data remains;
                    // a late object keeps this candidate pending instead.
                    // Keep maintenance alive until the terminal retention
                    // deadline, even when no new mutation occurs.  The
                    // scheduler will wake again and reap the claim when due.
                    if candidate.has_garbage_marker {
                        self.clear_garbage(&build_id).await?;
                    }
                    return Ok(true);
                }
                return Ok(true);
            }
            let stale_publishing =
                if candidate.has_publishing_marker {
                    self.read_build_claim(&build_id).await?.is_some_and(
                        |(bytes, _, last_modified)| match Self::claim_role(&bytes).as_deref() {
                            Some("released") => true,
                            Some("complete") => {
                                !candidate.has_build_object
                                    || Self::old_enough(last_modified, minimum_gc_age)
                            }
                            Some("reaping") => true,
                            Some("publishing") => Self::claim_is_stale(&bytes, last_modified),
                            // A garbage claim without garbage.json means the
                            // final marker deletion already happened but the
                            // terminal-role transition may not have. Keep it
                            // pending until that transition is observed.
                            Some("garbage") if !candidate.has_garbage_marker => true,
                            Some("garbage") => false,
                            _ => false,
                        },
                    )
                } else {
                    false
                };
            let markerless_orphan = !candidate.has_staging_marker
                && !candidate.has_garbage_fence
                && !candidate.has_publishing_marker
                && (candidate
                    .newest_object_modified
                    .is_some_and(|modified| Self::old_enough(Some(modified), minimum_gc_age))
                    || (self.write_mode == CatalogWriteMode::SingleProcess
                        && candidate.newest_object_modified.is_none()
                        && Self::old_enough(Self::build_id_time(&build_id), minimum_gc_age)));
            if candidate.stale_staging_old_enough || stale_publishing || markerless_orphan {
                return Ok(true);
            }
        }
        // The supplied current ID is only a scheduling hint.  A shared
        // publisher may have swapped Head while this listing was in flight;
        // keep maintenance alive so the next pass observes the new detached
        // build instead of treating a stale scan as idle.
        if self.current_build_id().await? != observed_current_build_id {
            return Ok(true);
        }
        Ok(false)
    }

    /// Runs GC while the caller already owns the relation-local single-process
    /// lock. Rebuild publication uses this variant so the GC scan/delete
    /// cannot interleave with its final Head swap.
    pub async fn garbage_collect_with_single_process_lock(
        &self,
        current_build_id: Option<&str>,
        minimum_gc_age: Duration,
    ) -> Result<Vec<String>> {
        self.reap_expired_terminal_tombstones().await?;
        self.recover_completed_empty_head_fence().await?;
        let observed_current_build_id = self.current_build_id().await?;
        // The caller's build ID is a scheduling hint only.  The exact Head
        // reread above is the sole authority used to skip a build.
        let _ = current_build_id;
        let current_build_id = observed_current_build_id.as_deref();
        let prefix = format!(
            "{}/_ugoite/derived/relations/{}/builds/",
            self.space_root, self.relation_id
        );
        let entries = self.list_derived_entries(&prefix).await?;
        #[derive(Default)]
        struct Candidate {
            garbage_marker_old_enough: bool,
            stale_staging_old_enough: bool,
            has_garbage_marker: bool,
            has_garbage_fence: bool,
            has_staging_marker: bool,
            has_publishing_marker: bool,
            has_build_object: bool,
            stale_publishing_old_enough: bool,
            complete_claim_old_enough: bool,
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
            if !is_garbage_marker && !is_staging_marker && !is_publishing_marker {
                candidate.has_build_object = true;
            }
            if let Some(modified) = modified {
                candidate.newest_object_modified = Some(
                    candidate
                        .newest_object_modified
                        .map_or(modified, |current| current.max(modified)),
                );
            }
        }
        // A process can crash after installing the empty-Head fence and
        // before the first garbage marker is written.  The fence itself is
        // then the durable recovery record; listing the build prefix alone is
        // not sufficient because the crash may have happened before any
        // marker object was created.
        if let Some((value, _, _)) = self.read_raw_exact().await? {
            if let Some((state, build_id)) = Self::head_fence(&value) {
                if state == "garbage_fence" && Some(build_id) != current_build_id {
                    let candidate = candidates.entry(build_id.to_string()).or_default();
                    candidate.has_garbage_fence = true;
                    candidate.garbage_marker_old_enough = true;
                }
            }
        }
        for (build_id, candidate) in &mut candidates {
            // A crash after publication claim creation can leave only
            // publishing.json behind. A live claim protects the build, while
            // a stale publishing claim is recoverable cleanup intent. A
            // terminal garbage claim with its marker remains a tombstone;
            // an unmarked stale garbage claim is recoverable cleanup intent.
            if candidate.has_publishing_marker {
                if let Some((bytes, _, last_modified)) = self.read_build_claim(build_id).await? {
                    candidate.stale_publishing_old_enough =
                        match Self::claim_role(&bytes).as_deref() {
                            Some("released") => true,
                            Some("complete") => {
                                candidate.complete_claim_old_enough = candidate.has_build_object
                                    && Self::old_enough(last_modified, minimum_gc_age);
                                candidate.complete_claim_old_enough
                            }
                            Some("staging") | Some("publishing") | Some("garbage") => {
                                Self::claim_is_stale(&bytes, last_modified)
                            }
                            Some("reaping") => true,
                            _ => false,
                        };
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
                && !candidate.has_garbage_fence
                && !candidate.has_staging_marker
                && !candidate.has_publishing_marker
                && (candidate
                    .newest_object_modified
                    .is_some_and(|modified| Self::old_enough(Some(modified), minimum_gc_age))
                    || (self.write_mode == CatalogWriteMode::SingleProcess
                        && candidate.newest_object_modified.is_none()
                        && Self::old_enough(Self::build_id_time(build_id), minimum_gc_age)));
        }
        let mut deleted = Vec::new();
        for (build_id, candidate) in candidates {
            if !candidate.has_build_object
                && self
                    .read_build_claim(&build_id)
                    .await?
                    .is_some_and(|(bytes, _, _)| {
                        matches!(
                            Self::claim_role(&bytes).as_deref(),
                            Some("complete") | Some("reaping")
                        )
                    })
            {
                if candidate.has_garbage_marker {
                    self.clear_garbage(&build_id).await?;
                }
                let _ = self.reap_terminal_claim(&build_id).await?;
                continue;
            }
            // A garbage marker is written after a build has either lost
            // publication or stopped being current. A stale staging marker is
            // also a durable cleanup candidate: it covers a process crash
            // between staging and the failure path that writes garbage.json.
            // Once garbage.json exists it is the cleanup record for this
            // build. Its own age is the grace-period boundary; an older
            // staging marker must not allow a freshly marked build to be
            // reclaimed early.
            let cleanup_old_enough = if candidate.has_garbage_marker || candidate.has_garbage_fence
            {
                candidate.garbage_marker_old_enough
            } else {
                candidate.stale_staging_old_enough
                    || candidate.orphan_old_enough
                    || candidate.stale_publishing_old_enough
                    || candidate.complete_claim_old_enough
            };
            if !cleanup_old_enough {
                continue;
            }
            // GC is discovery-only and must never decide authority from the
            // listing. Re-read the durable Head immediately before deleting
            // each candidate so a concurrent shared publisher is protected.
            if self.current_build_id().await?.as_deref() == Some(build_id.as_str()) {
                // A build can become current again after an uncertain
                // publication response left a conservative garbage marker.
                // Clear that marker while it is still current so its old
                // timestamp cannot shorten the next reader grace period.
                if candidate.has_garbage_marker {
                    self.clear_garbage(&build_id).await?;
                }
                if let Some(claim) = self.current_garbage_claim(&build_id).await? {
                    let _ = self.release_garbage_claim(&build_id, &claim).await;
                }
                continue;
            }
            // A delayed publisher can race marker-last cleanup and recreate
            // garbage.json after the terminal claim was written. An empty
            // prefix needs only marker cleanup; newly observed objects keep
            // the completed build eligible for a fresh GC claim.
            if candidate.has_garbage_marker
                && !candidate.has_build_object
                && self
                    .read_build_claim(&build_id)
                    .await?
                    .is_some_and(|(bytes, _, _)| {
                        matches!(
                            Self::claim_role(&bytes).as_deref(),
                            Some("complete") | Some("reaping")
                        )
                    })
            {
                self.clear_garbage(&build_id).await?;
                continue;
            }
            let needs_fresh_garbage_marker = !candidate.has_garbage_marker
                && !candidate.has_garbage_fence
                && (candidate.stale_staging_old_enough
                    || candidate.orphan_old_enough
                    || candidate.stale_publishing_old_enough
                    || candidate.complete_claim_old_enough);
            // The listing is only a hint. Recheck the staging timestamp after
            // discovery and immediately before claiming the build, so a
            // heartbeat that raced the listing cannot be converted into a
            // garbage claim from stale observations.
            if needs_fresh_garbage_marker
                && candidate.has_staging_marker
                && !self
                    .marker_old_enough(&self.staging_marker_path(&build_id), minimum_gc_age)
                    .await?
            {
                continue;
            }
            // Publication and GC claim the same object with conditional
            // create/replace. A fresh claim belongs to the other operation;
            // a stale claim can be atomically taken over for recovery.
            let Some(mut garbage_claim) = self.claim_build_for_garbage(&build_id).await? else {
                continue;
            };
            if needs_fresh_garbage_marker
                && candidate.has_staging_marker
                && !self
                    .marker_old_enough(&self.staging_marker_path(&build_id), minimum_gc_age)
                    .await?
            {
                let _ = self.release_garbage_claim(&build_id, &garbage_claim).await;
                continue;
            }
            if needs_fresh_garbage_marker {
                // The first pass only records cleanup intent. This makes the
                // marker timestamp the grace-period boundary even when an
                // old staging/publishing object is being reclaimed.
                self.mark_garbage(&build_id).await?;
                // The marker may not be visible in an immediately following
                // object listing on every backend. Keep its exact path in the
                // deletion set once this pass has created it, especially for
                // zero-age orphan cleanup that intentionally finishes in one
                // pass.
                if self.current_build_id().await?.as_deref() == Some(build_id.as_str()) {
                    let _ = self.release_garbage_claim(&build_id, &garbage_claim).await;
                    let _ = self.clear_garbage(&build_id).await;
                    continue;
                }
                // Markerless orphans have no durable cleanup timestamp, so a
                // zero-age maintenance pass may claim and delete them in one
                // pass. Staging/publishing recovery must always defer after
                // recording garbage.json: its timestamp is the reader grace
                // boundary, even when the caller explicitly selected zero.
                if !(minimum_gc_age.is_zero()
                    && (candidate.orphan_old_enough || candidate.complete_claim_old_enough))
                {
                    continue;
                }
            }
            // Claiming the build fences a delayed publisher, while fencing
            // the current Head makes the claim participate in the same CAS
            // sequence as publication.  If a publisher won first, this
            // returns false and the final Head is protected as current.
            let head_fence = match self.fence_head_before_garbage(&build_id).await? {
                GarbageHeadFence::CandidateIsCurrent | GarbageHeadFence::Contended => {
                    let _ = self.release_garbage_claim(&build_id, &garbage_claim).await;
                    continue;
                }
                fenced => fenced,
            };
            let build_prefix = self.builds_path(&build_id);
            let entries = self.list_derived_entries(&build_prefix).await?;
            let created_garbage_marker = needs_fresh_garbage_marker
                && (candidate.orphan_old_enough || candidate.complete_claim_old_enough)
                && minimum_gc_age.is_zero();
            let mut garbage_marker = (candidate.has_garbage_marker || created_garbage_marker)
                .then(|| self.garbage_marker_path(&build_id));
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
                    // The claim remains after cleanup as a terminal tombstone.
                    // It is transitioned to role=complete after garbage.json
                    // is removed, so a paused publisher cannot create a fresh
                    // claim for this already-deleted build.
                } else {
                    build_objects.push(entry.path().to_string());
                }
            }
            let mut fully_deleted = true;
            for path in build_objects {
                if !self
                    .renew_garbage_claim(&build_id, &mut garbage_claim)
                    .await?
                {
                    fully_deleted = false;
                    break;
                }
                // Re-check the Head for every object as a fail-closed guard
                // against a concurrent publication.
                if self.current_build_id().await?.as_deref() == Some(build_id.as_str()) {
                    fully_deleted = false;
                    break;
                }
                self.operator.delete(&path).await?;
            }
            if fully_deleted {
                // The final marker delete can occur after a long listing and
                // a zero-object cleanup pass. Renew and verify ownership at
                // the destructive boundary so an expired/replaced GC claim
                // cannot remove another writer's cleanup record.
                if !self
                    .renew_garbage_claim(&build_id, &mut garbage_claim)
                    .await?
                {
                    continue;
                }
                // Keep garbage.json until the build prefix is otherwise empty.
                // If this process crashes before this final delete, the marker
                // remains available for the next candidate-discovery pass.
                if self.current_build_id().await?.as_deref() == Some(build_id.as_str()) {
                    continue;
                }
                // The garbage marker is the final durable cleanup record. If
                // a publisher won the Head CAS after the previous check, the
                // marker must remain so the build is rediscovered safely. The
                // The claim is intentionally retained as a terminal tombstone,
                // fencing delayed publishers even after this marker is removed.
                if self.current_build_id().await?.as_deref() == Some(build_id.as_str()) {
                    continue;
                }
                if let Some(path) = garbage_marker {
                    // Marker-last is crash-safe even if a previous pass
                    // already removed the marker before crashing: the
                    // cleanup record is idempotent at this final step.
                    match self.operator.delete(&path).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                // Keep a durable terminal claim after marker-last cleanup so
                // a delayed publisher remains fenced. The explicit complete
                // role also makes the terminal tombstone invisible to pending
                // maintenance checks; a crash before this transition leaves
                // role=garbage and is recoverable on the next pass.
                if !self
                    .complete_garbage_claim(&build_id, &garbage_claim)
                    .await?
                {
                    continue;
                }
                if let GarbageHeadFence::Fenced {
                    empty_head: true,
                    etag: Some(etag),
                } = &head_fence
                {
                    // Do not remove the temporary first-Head fence. Convert
                    // it conditionally into an empty-head release marker so
                    // a new publisher can replace it only after every build
                    // object, garbage marker, and terminal claim are durable.
                    // A crash before this step is recovered by the next GC
                    // pass from the complete claim.
                    if !self.release_empty_head_fence(&build_id, etag).await? {
                        continue;
                    }
                }
                deleted.push(build_id);
            }
        }
        Ok(deleted)
    }

    /// Records the removed v1 materialization prefix as a grace-period GC
    /// candidate. The marker is created after the current Head swap, so an
    /// in-flight reader that pinned the legacy Head can finish before the
    /// prefix is reclaimed.
    pub async fn mark_legacy_materializations_garbage(&self) -> Result<()> {
        let entries = self
            .list_derived_entries(&self.legacy_materializations_prefix())
            .await?;
        if !entries
            .iter()
            .any(|entry| entry.metadata().mode() == EntryMode::FILE)
        {
            return Ok(());
        }
        match self
            .operator
            .write_options(
                &self.legacy_garbage_marker_path(),
                Self::build_marker_bytes(),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Deletes a detached v1 prefix only after its durable marker has aged
    /// past the same reader grace period as normal garbage builds. The marker
    /// is deleted last so a crash during recursive deletion leaves a retryable
    /// cleanup record.
    pub async fn garbage_collect_legacy_materializations(
        &self,
        minimum_gc_age: Duration,
    ) -> Result<bool> {
        let marker = self.legacy_garbage_marker_path();
        if !self.operator.exists(&marker).await? {
            return Ok(false);
        }
        if !self.marker_old_enough(&marker, minimum_gc_age).await? {
            return Ok(true);
        }
        let entries = self
            .list_derived_entries(&self.legacy_materializations_prefix())
            .await?;
        for entry in entries {
            if entry.metadata().mode() == EntryMode::FILE {
                self.operator.delete(entry.path()).await?;
            }
        }
        self.operator.delete(&marker).await?;
        Ok(false)
    }

    pub async fn read_exact(&self) -> Result<Option<ExactDerivedRelationHead>> {
        let Some((value, bytes, etag)) = self.read_raw_exact().await? else {
            return Ok(None);
        };
        if Self::head_fence(&value).is_some() {
            return Ok(None);
        }
        let head: DerivedRelationHead = serde_json::from_value(value.clone()).map_err(|error| {
            if is_legacy_derived_head(&value) {
                LegacyDerivedRelationHead.into()
            } else {
                anyhow!("decode DerivedRelation Head: {error}")
            }
        })?;
        validate_derived_head_checksum(&head)?;
        self.validate_derived_head_identity(&head).await?;
        Ok(Some(ExactDerivedRelationHead { head, bytes, etag }))
    }

    pub async fn read_raw_for_rebuild(&self) -> Result<Option<RawDerivedRelationHead>> {
        let Some((bytes, etag)) = self.read_raw_bytes_exact().await? else {
            return Ok(None);
        };
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            if Self::head_fence(&value).is_some() {
                return Ok(None);
            }
        }
        Ok(Some(RawDerivedRelationHead { bytes, etag }))
    }

    async fn read_raw_bytes_exact(&self) -> Result<Option<(Vec<u8>, Option<String>)>> {
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
                Ok(bytes) => return Ok(Some((bytes.to_vec(), etag))),
                Err(error) if error.kind() == ErrorKind::ConditionNotMatch && attempt < 2 => {
                    continue
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact raw DerivedRelation Head reads always return or continue")
    }

    async fn validate_derived_head_identity(&self, head: &DerivedRelationHead) -> Result<()> {
        if head.format_version != 1 {
            return Err(anyhow!(
                "DerivedRelation Head format_version is unsupported"
            ));
        }
        if head.generation == 0
            || head.definition_version == 0
            || head.compatibility_epoch == 0
            || head.definition_fingerprint.trim().is_empty()
            || head.producer_id.trim().is_empty()
            || head.producer_fingerprint.trim().is_empty()
            || head.input_digest.trim().is_empty()
        {
            return Err(anyhow!(
                "DerivedRelation Head contains an incomplete identity coordinate"
            ));
        }
        let relation_id = Uuid::parse_str(&head.relation_id)
            .context("DerivedRelation Head relation_id is not a UUID")?;
        if relation_id != self.relation_id {
            return Err(anyhow!(
                "DerivedRelation Head relation_id does not match its path"
            ));
        }
        let build_id = Uuid::parse_str(&head.build_id)
            .context("DerivedRelation Head build_id is not a UUIDv7")?;
        if build_id.get_version_num() != 7 || (build_id.as_bytes()[8] & 0xc0) != 0x80 {
            return Err(anyhow!("DerivedRelation Head build_id must be UUIDv7"));
        }
        let space_uid = Uuid::parse_str(&head.space_id)
            .context("DerivedRelation Head space_id is not a UUIDv7")?;
        if space_uid.get_version_num() != 7 || (space_uid.as_bytes()[8] & 0xc0) != 0x80 {
            return Err(anyhow!("DerivedRelation Head space_id must be UUIDv7"));
        }
        Uuid::parse_str(&head.table_uuid)
            .context("DerivedRelation Head table_uuid is not a UUID")?;
        if !head
            .table_identifier
            .as_object()
            .is_some_and(|object| !object.is_empty())
        {
            return Err(anyhow!(
                "DerivedRelation Head table_identifier must be a non-empty object"
            ));
        }
        let storage = IcebergStorageConfig::from_operator(&self.operator)?;
        let warehouse_uri = if storage.warehouse_uri == "memory:" {
            "memory:///".to_string()
        } else {
            format!("{}/", storage.warehouse_uri.trim_end_matches('/'))
        };
        let expected_prefix = format!(
            "{}{}/_ugoite/derived/relations/{}/builds/{}",
            warehouse_uri, self.space_root, self.relation_id, head.build_id
        );
        let expected_url = url::Url::parse(&expected_prefix)
            .context("parse expected DerivedRelation metadata URI")?;
        let actual_url = url::Url::parse(&head.metadata_location)
            .context("parse DerivedRelation Head metadata_location")?;
        let raw_has_dot_segment = head.metadata_location.split('/').any(|segment| {
            let segment = segment
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            matches!(segment.as_str(), "." | ".." | "%2e" | "%2e%2e")
        });
        let expected_segments = expected_url
            .path_segments()
            .map(|segments| segments.collect::<Vec<_>>())
            .unwrap_or_default();
        let actual_segments = actual_url
            .path_segments()
            .map(|segments| segments.collect::<Vec<_>>())
            .unwrap_or_default();
        let exact_authority = actual_url.scheme() == expected_url.scheme()
            && actual_url.username() == expected_url.username()
            && actual_url.password() == expected_url.password()
            && actual_url.host() == expected_url.host()
            && actual_url.port_or_known_default() == expected_url.port_or_known_default();
        let has_safe_metadata_path = actual_segments.len() > expected_segments.len()
            && actual_segments[..expected_segments.len()] == expected_segments[..]
            && actual_segments[expected_segments.len()..]
                .iter()
                .all(|segment| !matches!(*segment, "." | ".."));
        if head.metadata_location.trim().is_empty()
            || !exact_authority
            || actual_url.query().is_some()
            || actual_url.fragment().is_some()
            || raw_has_dot_segment
            || !has_safe_metadata_path
        {
            return Err(anyhow!(
                "DerivedRelation Head metadata_location is not bound to its Space, relation, and build"
            ));
        }

        // UUID-addressed Spaces are structurally bound even for the generic
        // storage adapter. Legacy slug-addressed Spaces are bound by the
        // authoritative metadata check in the AssetText reader.
        if let Some(directory_id) = self.space_root.strip_prefix("spaces/") {
            if let Ok(directory_uid) = Uuid::parse_str(directory_id) {
                if directory_uid.get_version_num() == 7 && directory_uid != space_uid {
                    return Err(anyhow!(
                        "DerivedRelation Head space_id does not match its Space directory"
                    ));
                }
            }
        }
        let metadata_path = format!("{}/meta.json", self.space_root);
        match self.operator.read(&metadata_path).await {
            Ok(bytes) => {
                let metadata: serde_json::Value = serde_json::from_slice(&bytes.to_vec())
                    .context("decode authoritative Space metadata for DerivedRelation Head")?;
                let authoritative_uid = metadata
                    .get("space_uid")
                    .and_then(serde_json::Value::as_str)
                    .context("authoritative Space metadata has no space_uid")?
                    .parse::<Uuid>()?;
                if authoritative_uid.get_version_num() != 7 || authoritative_uid != space_uid {
                    return Err(anyhow!(
                        "DerivedRelation Head space_id does not match authoritative Space metadata"
                    ));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(anyhow!(
                    "authoritative Space metadata is missing for DerivedRelation Head"
                ));
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Returns the current build coordinate for GC authority checks. A legacy
    /// v1 Head still pins its old materialization prefix, but it does not pin
    /// any build under the current `builds/` layout; it must therefore be
    /// treated as an empty current-build coordinate while remaining untouched.
    async fn current_build_id(&self) -> Result<Option<String>> {
        match self.read_exact().await {
            Ok(head) => Ok(head.map(|head| head.head.build_id)),
            Err(error) if error.downcast_ref::<LegacyDerivedRelationHead>().is_some() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reads the disposable v1 Head without treating it as a current
    /// coordinate. Shared rebuilds use the returned ETag to replace it after
    /// the new immutable build has been fully validated.
    pub async fn read_legacy_exact(&self) -> Result<Option<ExactLegacyDerivedRelationHead>> {
        let Some((value, bytes, etag)) = self.read_raw_exact().await? else {
            return Ok(None);
        };
        if Self::head_fence(&value).is_some() {
            return Ok(None);
        }
        if !is_legacy_derived_head(&value) {
            return Ok(None);
        }
        Ok(Some(ExactLegacyDerivedRelationHead {
            generation: value
                .get("generation")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            bytes,
            etag,
        }))
    }

    async fn read_raw_exact(&self) -> Result<Option<(serde_json::Value, Vec<u8>, Option<String>)>> {
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
                    return Ok(Some((value, bytes, etag)));
                }
                Err(error) if error.kind() == ErrorKind::ConditionNotMatch && attempt < 2 => {
                    continue
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact derived Head reads always return or continue")
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
            // Detach the old Head, but retain its immutable prefix behind a
            // durable grace-period marker so an already-open legacy reader is
            // not broken by this explicit format discard.
            self.mark_legacy_materializations_garbage().await?;
            self.operator.delete(&self.head_path()).await?;
        }
        Ok(())
    }

    pub async fn create(&self, head: &DerivedRelationHead) -> Result<()> {
        self.validate_derived_head_identity(head).await?;
        self.ensure_build_publishable(&head.build_id).await?;
        let bytes = canonical_head_bytes(head)?;
        match self.write_mode {
            CatalogWriteMode::Shared => match self.read_raw_exact().await? {
                Some((value, _, Some(etag)))
                    if Self::head_fence(&value)
                        .is_some_and(|(state, _)| state == "head_fence_released") =>
                {
                    let Some((_, _)) = Self::head_fence(&value) else {
                        unreachable!("released Head fence was checked above")
                    };
                    // A released empty-head fence can be replaced by a new
                    // build. The publishing-claim check below still rejects
                    // the build that GC has already completed.
                    let Some((claim, _, _)) = self.read_build_claim(&head.build_id).await? else {
                        return Err(anyhow!(
                            "released empty DerivedRelation Head fence has no build claim"
                        ));
                    };
                    if Self::claim_role(&claim).as_deref() != Some("publishing") {
                        return Err(anyhow!(
                            "released empty DerivedRelation Head fence is no longer publishable"
                        ));
                    }
                    self.operator
                        .write_options(
                            &self.head_path(),
                            bytes,
                            WriteOptions {
                                if_match: Some(etag),
                                ..Default::default()
                            },
                        )
                        .await?;
                }
                Some((value, _, None))
                    if Self::head_fence(&value)
                        .is_some_and(|(state, _)| state == "head_fence_released") =>
                {
                    return Err(anyhow!(
                        "shared empty DerivedRelation Head fence did not return an ETag"
                    ));
                }
                _ => {
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
            },
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
        self.validate_derived_head_identity(head).await?;
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
        let (heartbeat, heartbeat_lost) = self.start_publishing_claim_heartbeat(&head.build_id);
        let result = match expected {
            None => self.create(head).await,
            Some(expected) => self.replace(expected.etag.as_deref(), head).await,
        };
        heartbeat.abort();
        self.finish_publishing_claim(
            &head.build_id,
            result,
            heartbeat_lost.load(Ordering::Acquire),
        )
        .await
    }

    /// Replaces a disposable legacy Head after a complete build has been
    /// staged. Shared backends use the legacy Head's exact ETag, so a concurrent
    /// writer that already moved the relation to the current format wins the
    /// race and this build becomes garbage instead of overwriting it.
    pub async fn publish_over_legacy(
        &self,
        expected: &ExactLegacyDerivedRelationHead,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        if self.write_mode != CatalogWriteMode::Shared {
            return Err(anyhow!(
                "legacy DerivedRelation replacement requires shared mode"
            ));
        }
        let etag = expected
            .etag
            .as_deref()
            .context("shared legacy DerivedRelation replacement requires an ETag")?;
        self.validate_derived_head_identity(head).await?;
        let bytes = canonical_head_bytes(head)?;
        self.begin_publishing(&head.build_id).await?;
        let (heartbeat, heartbeat_lost) = self.start_publishing_claim_heartbeat(&head.build_id);
        let result = self
            .operator
            .write_options(
                &self.head_path(),
                bytes,
                WriteOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from);
        heartbeat.abort();
        self.finish_publishing_claim(
            &head.build_id,
            result,
            heartbeat_lost.load(Ordering::Acquire),
        )
        .await
    }

    /// Replaces a disposable legacy Head while the caller already owns the
    /// single-process relation lock. The legacy coordinate remains readable
    /// until this exact swap completes; GC is marked only afterward.
    pub async fn publish_over_legacy_with_single_process_lock(
        &self,
        expected: &ExactLegacyDerivedRelationHead,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        if self.write_mode != CatalogWriteMode::SingleProcess {
            return Err(anyhow!(
                "single-process legacy DerivedRelation replacement requires local mode"
            ));
        }
        self.validate_derived_head_identity(head).await?;
        self.begin_publishing(&head.build_id).await?;
        let result = async {
            let Some((_, current_bytes, current_etag)) = self.read_raw_exact().await? else {
                return Err(anyhow!("legacy DerivedRelation Head disappeared"));
            };
            if current_bytes != expected.bytes
                || (expected.etag.is_some() && current_etag != expected.etag)
            {
                return Err(anyhow!("legacy DerivedRelation Head changed"));
            }
            self.operator
                .write(&self.head_path(), canonical_head_bytes(head)?)
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        }
        .await;
        self.finish_publishing_claim(&head.build_id, result, false)
            .await
    }

    /// Publish while the caller already owns [`Self::single_process_lock`].
    /// This is used by a full rebuild so the relation mutex spans source scan,
    /// build, validation, and swap without self-deadlocking on Head I/O.
    pub async fn publish_with_single_process_lock(
        &self,
        expected: Option<&ExactDerivedRelationHead>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        self.validate_derived_head_identity(head).await?;
        self.begin_publishing(&head.build_id).await?;
        if self.write_mode != CatalogWriteMode::SingleProcess {
            let (heartbeat, heartbeat_lost) = self.start_publishing_claim_heartbeat(&head.build_id);
            let result = match expected {
                None => self.create(head).await,
                Some(expected) => self.replace(expected.etag.as_deref(), head).await,
            };
            heartbeat.abort();
            return self
                .finish_publishing_claim(
                    &head.build_id,
                    result,
                    heartbeat_lost.load(Ordering::Acquire),
                )
                .await;
        }
        let bytes = canonical_head_bytes(head)?;
        let result = async {
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
        .await;
        self.finish_publishing_claim(&head.build_id, result, false)
            .await
    }

    pub async fn publish_over_corrupt(
        &self,
        expected: &RawDerivedRelationHead,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        if self.write_mode == CatalogWriteMode::SingleProcess {
            let _guard = self.serializer.lock().await;
            return self
                .publish_over_corrupt_with_single_process_lock(expected, head)
                .await;
        }
        self.validate_derived_head_identity(head).await?;
        self.ensure_build_publishable(&head.build_id).await?;
        let etag = expected
            .etag
            .as_deref()
            .context("shared corrupt DerivedRelation Head replacement requires an ETag")?;
        self.begin_publishing(&head.build_id).await?;
        let (heartbeat, heartbeat_lost) = self.start_publishing_claim_heartbeat(&head.build_id);
        let result = self
            .operator
            .write_options(
                &self.head_path(),
                canonical_head_bytes(head)?,
                WriteOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from);
        heartbeat.abort();
        self.finish_publishing_claim(
            &head.build_id,
            result,
            heartbeat_lost.load(Ordering::Acquire),
        )
        .await
    }

    pub async fn publish_over_corrupt_with_single_process_lock(
        &self,
        expected: &RawDerivedRelationHead,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        self.validate_derived_head_identity(head).await?;
        self.ensure_build_publishable(&head.build_id).await?;
        self.begin_publishing(&head.build_id).await?;
        let result = async {
            let Some((current_bytes, current_etag)) = self.read_raw_bytes_exact().await? else {
                return Err(anyhow!("corrupt DerivedRelation Head disappeared"));
            };
            if current_bytes != expected.bytes
                || (expected.etag.is_some() && current_etag != expected.etag)
            {
                return Err(anyhow!("corrupt DerivedRelation Head changed"));
            }
            self.operator
                .write(&self.head_path(), canonical_head_bytes(head)?)
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        }
        .await;
        self.finish_publishing_claim(&head.build_id, result, false)
            .await
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

    pub fn mutation_permit(&self) -> Result<CatalogMutationPermit> {
        if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            return Ok(CatalogMutationPermit {
                store_key: self.mutation_store_key(),
            });
        }
        Err(anyhow!(
            "STORAGE_MUTATION_UNAVAILABLE: non-local authoritative Catalog writes are unavailable in v0.1"
        ))
    }

    fn mutation_store_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.operator.info().scheme(),
            self.operator.info().name(),
            self.operator.info().root(),
            self.space_root
        )
    }

    fn require_mutation_permit(&self, permit: &CatalogMutationPermit) -> Result<()> {
        if permit.store_key == self.mutation_store_key() {
            Ok(())
        } else {
            Err(anyhow!("Catalog mutation permit belongs to another store"))
        }
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
                .any(|key| key == cache_key)
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
            let mut cache = SHARED_WRITE_VERIFICATIONS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .expect("shared-write verification cache poisoned");
            // Capability evidence is only an optimization. Bound the key
            // registry and never retain a strong Operator just to keep it
            // warm; an evicted backend is safely re-probed on demand.
            if !cache.iter().any(|key| key == &cache_key) {
                const MAX_SHARED_WRITE_VERIFICATIONS: usize = 256;
                if cache.len() >= MAX_SHARED_WRITE_VERIFICATIONS {
                    cache.remove(0);
                }
                cache.push(cache_key);
            }
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

    pub async fn create_head(&self, permit: &CatalogMutationPermit, bytes: Vec<u8>) -> Result<()> {
        self.require_mutation_permit(permit)?;
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

    pub async fn replace_head(
        &self,
        permit: &CatalogMutationPermit,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
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
        permit: &CatalogMutationPermit,
        asset_id: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
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
        permit: &CatalogMutationPermit,
        asset_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
        self.replace_exact_object(
            &self.catalog_path(&format!("asset-lifecycle/{asset_id}")),
            etag,
            bytes,
        )
        .await
    }

    pub async fn delete_asset_lifecycle_marker(
        &self,
        permit: &CatalogMutationPermit,
        asset_id: &str,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
        let path = self.catalog_path(&format!("asset-lifecycle/{asset_id}"));
        match self.operator.delete(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Lists the durable Asset lifecycle records used by the physical-blob
    /// sweeper.  The Catalog Head remains the authority for deletion state;
    /// this bounded listing only discovers committed tombstones whose object
    /// cleanup may have been interrupted after the authoritative commit.
    pub async fn list_asset_lifecycle_markers(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let prefix = self.catalog_path("asset-lifecycle/");
        let mut lister = self.operator.lister_with(&prefix).recursive(false).await?;
        let mut examined = 0usize;
        let mut markers = Vec::new();
        while let Some(entry) = lister.try_next().await? {
            examined = examined.saturating_add(1);
            if examined > MAX_ASSET_LIFECYCLE_SCAN_ENTRIES {
                return Err(anyhow!(
                    "Asset lifecycle marker prefix exceeds the {MAX_ASSET_LIFECYCLE_SCAN_ENTRIES}-entry safety bound"
                ));
            }
            if entry.metadata().mode() != EntryMode::FILE {
                continue;
            }
            let Some(asset_id) = entry.path().rsplit('/').next().filter(|id| !id.is_empty()) else {
                continue;
            };
            markers.push((
                asset_id.to_string(),
                self.operator.read(entry.path()).await?.to_vec(),
            ));
        }
        Ok(markers)
    }

    /// Deletes only the exact Asset object key.  Callers must first establish
    /// that its durable lifecycle marker is committed; this operation is a
    /// repair step and never decides whether an Asset is logically deleted.
    pub async fn delete_asset_blob(
        &self,
        permit: &CatalogMutationPermit,
        asset_id: &str,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
        let path = self.space_path(&format!("assets/{asset_id}"));
        match self.operator.delete(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn create_publication(
        &self,
        permit: &CatalogMutationPermit,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
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

    pub async fn create_command_receipt(
        &self,
        permit: &CatalogMutationPermit,
        command_id: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
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
        permit: &CatalogMutationPermit,
        command_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
        self.replace_exact_object(&self.command_receipt_path(command_id), etag, bytes)
            .await
    }

    pub async fn create_checkpoint(
        &self,
        permit: &CatalogMutationPermit,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.require_mutation_permit(permit)?;
        if !self.operator.info().capability().write_with_if_not_exists {
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
        let capabilities = self.operator.info().capability();
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
            Arc::as_ptr(self.operator.service()),
        )
    }

    pub fn backend_capabilities(&self) -> CatalogBackendCapabilities {
        let capabilities = self.operator.info().capability();
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
static SHARED_WRITE_VERIFICATIONS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

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
    let atomic_write_dir = local_atomic_write_dir(root)?;
    let mut builder = Fs::default().root(root);
    std::fs::create_dir_all(&atomic_write_dir).with_context(|| {
        format!(
            "create same-filesystem atomic write directory {}",
            atomic_write_dir.display()
        )
    })?;
    set_owner_only_directory(&atomic_write_dir)?;
    builder = builder.atomic_write_dir(atomic_write_dir.to_string_lossy().as_ref());
    let op = Operator::new(builder)?;
    Ok(op)
}

fn local_atomic_write_dir(root: &str) -> Result<std::path::PathBuf> {
    // Atomic writes target Space objects below root/spaces. Keep the
    // temporary directory on that exact filesystem, including when `spaces`
    // is a separate mount or does not exist yet.
    if root == "/" {
        let spaces = Path::new(root).join("spaces");
        if spaces.exists() {
            return Ok(spaces.join(".ugoite-atomic-writes"));
        }
        let temp = std::env::temp_dir();
        if same_filesystem(Path::new(root), &temp) {
            return Ok(temp.join(format!(".ugoite-atomic-writes-{}", std::process::id())));
        }
        bail!("cannot configure same-filesystem atomic writes for local root /");
    }
    Ok(Path::new(root).join("spaces").join(".ugoite-atomic-writes"))
}

#[cfg(unix)]
fn same_filesystem(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(first)
        .and_then(|first| std::fs::metadata(second).map(|second| first.dev() == second.dev()))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn same_filesystem(_first: &Path, _second: &Path) -> bool {
    true
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<()> {
    Ok(())
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
        let op = Operator::new(Memory::default())?;
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
        return Ok(Operator::new(builder)?);
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

    async fn write_local_json_atomic(&self, path: &str, data: &[u8]) -> Result<bool> {
        let Some(target) = self.local_path(path) else {
            return Ok(false);
        };
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("storage path has no parent: {path}"))?;
        tokio::fs::create_dir_all(parent).await?;
        let file_name = target
            .file_name()
            .ok_or_else(|| anyhow!("storage path has no file name: {path}"))?
            .to_string_lossy();
        let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
        let result = async {
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true).truncate(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary).await?;
            file.write_all(data).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&temporary, &target).await?;
            #[cfg(unix)]
            if let Ok(directory) = tokio::fs::File::open(parent).await {
                directory.sync_all().await?;
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result?;
        Ok(true)
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

        if !self.operator.info().capability().write_with_if_not_exists {
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

    async fn write_json<T>(&self, path: &str, value: &T) -> Result<()>
    where
        T: Serialize + Sync,
    {
        let data = serde_json::to_vec_pretty(value)?;
        if self.write_local_json_atomic(path, &data).await? {
            return Ok(());
        }
        self.operator.write(path, data).await?;
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
        StorageBackend, MAX_DERIVED_TERMINAL_TOMBSTONES_PER_PASS,
    };
    use anyhow::Result;
    use futures::future::join_all;
    use futures::TryStreamExt;
    use opendal::services::Memory;
    use opendal::Operator;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn test_build_id() -> String {
        Uuid::now_v7().to_string()
    }

    async fn write_test_space_metadata(operator: &Operator, space_uid: Uuid) -> Result<()> {
        operator
            .write(
                "spaces/demo/meta.json",
                serde_json::to_vec(&serde_json::json!({"space_uid": space_uid}))?,
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn operator_from_uri_supports_fs_and_memory() -> Result<()> {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir()?;
        let fs_uri = format!("fs://{}", temp_dir.path().display());
        let fs_operator = operator_from_uri(&fs_uri)?;
        fs_operator
            .write("hello.txt", b"hello world".to_vec())
            .await?;
        let fs_bytes = fs_operator.read("hello.txt").await?.to_vec();
        assert_eq!(fs_bytes, b"hello world");
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(temp_dir.path().join("spaces/.ugoite-atomic-writes"))?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

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
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/demo")?;

        assert!(store.read_exact_head().await?.is_none());
        let permit = store.mutation_permit()?;
        store.create_head(&permit, b"first".to_vec()).await?;
        let first = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(first.bytes, b"first");

        store
            .replace_head(&permit, None, b"second".to_vec())
            .await?;
        let second = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(second.bytes, b"second");

        Ok(())
    }

    #[tokio::test]
    async fn raw_catalog_writers_require_a_same_store_local_permit() -> Result<()> {
        let remote = SpaceCatalogStore::new(
            operator_from_uri("s3://ugoite-test-bucket/catalog-boundary")?,
            "spaces/remote",
        )?;
        let local = SpaceCatalogStore::new(
            operator_from_uri("memory://catalog-boundary-permit")?,
            "spaces/local",
        )?;
        let permit = local.mutation_permit()?;

        assert!(remote.create_head(&permit, b"head".to_vec()).await.is_err());
        assert!(remote
            .replace_head(&permit, Some("etag"), b"head".to_vec())
            .await
            .is_err());
        assert!(remote
            .create_asset_lifecycle_marker(&permit, "asset", b"marker".to_vec())
            .await
            .is_err());
        assert!(remote
            .replace_asset_lifecycle_marker(&permit, "asset", None, b"marker".to_vec())
            .await
            .is_err());
        assert!(remote
            .delete_asset_lifecycle_marker(&permit, "asset")
            .await
            .is_err());
        assert!(remote.delete_asset_blob(&permit, "asset").await.is_err());
        assert!(remote
            .create_publication(&permit, "publication.json", b"publication".to_vec())
            .await
            .is_err());
        assert!(remote
            .create_command_receipt(&permit, "command", b"receipt".to_vec())
            .await
            .is_err());
        assert!(remote
            .replace_command_receipt(&permit, "command", None, b"receipt".to_vec())
            .await
            .is_err());
        assert!(remote
            .create_checkpoint(&permit, "checkpoint", b"checkpoint".to_vec())
            .await
            .is_err());
        assert!(remote.mutation_permit().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn shared_catalog_mode_fails_closed_without_an_exact_etag_contract() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
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
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA001);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id)
            .single_process();
        let space_uid = Uuid::now_v7();
        write_test_space_metadata(&operator, space_uid).await?;
        let first_build_id = test_build_id();
        let first = DerivedRelationHead {
            format_version: 1,
            space_id: space_uid.to_string(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            build_id: first_build_id.clone(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: Uuid::now_v7().to_string(),
            metadata_location: format!(
                "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{first_build_id}/metadata.json"
            ),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input-a".into(),
            source_coordinate: serde_json::json!({"catalog_head_sha256":null}),
            head_fence: String::new(),
            checksum: String::new(),
        };
        store.mark_staging(&first.build_id).await?;
        store.create(&first).await?;
        let exact_first = store.read_exact().await?.expect("derived Head");
        assert!(!exact_first.head.checksum.is_empty());

        let escaped_build_id = test_build_id();
        let mut escaped = first.clone();
        escaped.generation = 2;
        escaped.build_id = escaped_build_id.clone();
        escaped.metadata_location = format!(
            "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{escaped_build_id}/../outside/metadata.json"
        );
        store.mark_staging(&escaped.build_id).await?;
        let escape_error = store
            .replace(None, &escaped)
            .await
            .expect_err("metadata must not escape the relation build prefix");
        assert!(escape_error.to_string().contains("metadata_location"));

        let mut second = first.clone();
        second.generation = 2;
        second.build_id = test_build_id();
        second.metadata_location = format!(
            "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{}/metadata.json",
            second.build_id
        );
        store.mark_staging(&second.build_id).await?;
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
        let operator = Operator::new(Memory::default())?;
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
        let legacy = store
            .read_legacy_exact()
            .await?
            .expect("legacy Head remains an exact disposable coordinate");
        assert_eq!(legacy.generation, 1);
        store.invalidate_legacy_head().await?;
        assert!(!operator.exists(&store.head_path()).await?);
        assert!(operator.exists(&legacy_data_path).await?);
        assert!(operator.exists(&store.legacy_garbage_marker_path()).await?);
        Ok(())
    }

    #[tokio::test]
    async fn garbage_age_starts_when_build_is_marked_garbage() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
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
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA008);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let partial = format!("{}/data/partial.parquet", store.builds_path(&build_id));
        store.mark_staging(&build_id).await?;
        operator.write(&partial, b"partial".to_vec()).await?;
        store.mark_garbage(&build_id).await?;
        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec![build_id]);
        assert!(!operator.exists(&partial).await?);
        Ok(())
    }

    #[tokio::test]
    async fn garbage_collection_retains_terminal_claim_after_marker_cleanup() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00C);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let data = format!("{}/data/old.parquet", store.builds_path(&build_id));
        operator.write(&data, b"old".to_vec()).await?;
        store.mark_staging(&build_id).await?;
        store.mark_garbage(&build_id).await?;

        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec![build_id.clone()]
        );
        assert!(!operator.exists(&data).await?);
        assert!(
            !operator
                .exists(&store.garbage_marker_path(&build_id))
                .await?
        );
        // The terminal garbage claim fences a publisher that was paused
        // before the GC claim was created, even after garbage.json is gone.
        assert!(
            operator
                .exists(&store.publishing_marker_path(&build_id))
                .await?
        );
        assert!(store
            .ensure_build_publishable(&build_id)
            .await
            .expect_err("a terminal garbage claim must fence publication")
            .to_string()
            .contains("terminal garbage claim"));
        assert!(store
            .begin_publishing(&build_id)
            .await
            .expect_err("a reclaimed build must stay fenced")
            .to_string()
            .contains("no longer staged"));
        // A delayed loser may report the same build after cleanup. It must
        // not recreate garbage.json or wake GC forever.
        store.mark_garbage(&build_id).await?;
        assert!(
            !operator
                .exists(&store.garbage_marker_path(&build_id))
                .await?
        );
        // The terminal claim remains pending until its retention deadline so
        // a scheduler can wake without a new mutation and reap the fence.
        assert!(store.has_pending_garbage(None, Duration::ZERO).await?);
        Ok(())
    }

    #[tokio::test]
    async fn late_producer_objects_after_terminal_gc_are_rediscovered() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA011);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = Uuid::now_v7().to_string();
        let first = format!("{}/data/first.parquet", store.builds_path(&build_id));
        operator.write(&first, b"first".to_vec()).await?;
        store.mark_garbage(&build_id).await?;
        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec![build_id.clone()]
        );

        // Simulate a producer whose already-started object write completed
        // after marker-last cleanup and the terminal claim transition.
        let late = format!("{}/data/late.parquet", store.builds_path(&build_id));
        operator.write(&late, b"late".to_vec()).await?;
        store.mark_garbage(&build_id).await?;
        assert!(
            operator
                .exists(&store.garbage_marker_path(&build_id))
                .await?
        );
        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec![build_id]
        );
        assert!(!operator.exists(&late).await?);
        Ok(())
    }

    #[tokio::test]
    async fn stale_staging_build_gets_durable_cleanup_intent_and_is_collectable() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA009);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let partial = format!("{}/data/crashed.parquet", store.builds_path(&build_id));
        store.mark_staging(&build_id).await?;
        operator.write(&partial, b"crashed".to_vec()).await?;

        assert!(store
            .garbage_collect(None, Duration::ZERO)
            .await?
            .is_empty());
        assert!(operator.exists(&partial).await?);
        assert!(
            operator
                .exists(&store.garbage_marker_path(&build_id))
                .await?
        );
        let deleted = store.garbage_collect(None, Duration::ZERO).await?;
        assert_eq!(deleted, vec![build_id.clone()]);
        assert!(!operator.exists(&partial).await?);
        assert!(
            !operator
                .exists(&format!("{}/garbage.json", store.builds_path(&build_id)))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn pending_gc_distinguishes_deferred_cleanup_from_terminal_fence() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00F);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let partial = format!("{}/data/deferred.parquet", store.builds_path(&build_id));
        store.mark_staging(&build_id).await?;
        operator.write(&partial, b"deferred".to_vec()).await?;

        assert!(store
            .garbage_collect(None, Duration::ZERO)
            .await?
            .is_empty());
        assert!(store.has_pending_garbage(None, Duration::ZERO).await?);

        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec![build_id]
        );
        // Cleanup is complete, but the terminal publication fence is still
        // intentionally pending until its bounded retention window expires.
        assert!(store.has_pending_garbage(None, Duration::ZERO).await?);
        Ok(())
    }

    #[tokio::test]
    async fn fresh_garbage_marker_preserves_staging_grace_period() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00B);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let data = format!("{}/data/crashed.parquet", store.builds_path(&build_id));
        store.mark_staging(&build_id).await?;
        operator.write(&data, b"crashed".to_vec()).await?;
        store.mark_garbage(&build_id).await?;

        assert!(store
            .garbage_collect(None, Duration::from_secs(3600))
            .await?
            .is_empty());
        assert!(operator.exists(&data).await?);
        assert!(
            operator
                .exists(&store.garbage_marker_path(&build_id))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn garbage_marker_age_uses_persisted_timestamp_on_memory_backend() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00D);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        let data = format!("{}/data/old.parquet", store.builds_path(&build_id));
        operator.write(&data, b"old".to_vec()).await?;
        let marked_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .saturating_sub(3600);
        operator
            .write(
                &store.garbage_marker_path(&build_id),
                serde_json::to_vec(&serde_json::json!({ "marked_at": marked_at }))?,
            )
            .await?;

        assert_eq!(
            store
                .garbage_collect(None, Duration::from_secs(1800))
                .await?,
            vec![build_id]
        );
        assert!(!operator.exists(&data).await?);
        Ok(())
    }

    #[tokio::test]
    async fn legacy_materializations_use_a_grace_period_and_marker_last_cleanup() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00F);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let legacy_data = format!("{}/metadata.json", store.legacy_materializations_prefix());
        operator.write(&legacy_data, b"legacy".to_vec()).await?;

        store.mark_legacy_materializations_garbage().await?;
        assert!(
            store
                .garbage_collect_legacy_materializations(Duration::from_secs(3600))
                .await?
        );
        assert!(operator.exists(&legacy_data).await?);
        assert!(operator.exists(&store.legacy_garbage_marker_path()).await?);

        operator
            .write(
                &store.legacy_garbage_marker_path(),
                serde_json::to_vec(&serde_json::json!({
                    "marked_at": SystemTime::now()
                        .duration_since(UNIX_EPOCH)?
                        .as_secs()
                        .saturating_sub(3600)
                }))?,
            )
            .await?;
        assert!(
            !store
                .garbage_collect_legacy_materializations(Duration::from_secs(1800))
                .await?
        );
        assert!(!operator.exists(&legacy_data).await?);
        assert!(!operator.exists(&store.legacy_garbage_marker_path()).await?);
        Ok(())
    }

    #[tokio::test]
    async fn staging_heartbeat_keeps_a_persisted_gc_timestamp() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA00E);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        store.mark_staging(&build_id).await?;
        store.renew_staging(&build_id).await?;

        let marker = operator
            .read(&store.staging_marker_path(&build_id))
            .await?
            .to_vec();
        assert!(DerivedRelationHeadStore::marker_time(&marker).is_some());
        Ok(())
    }

    #[tokio::test]
    async fn staging_heartbeat_fails_after_its_marker_is_reclaimed() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA01E);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let build_id = test_build_id();
        store.mark_staging(&build_id).await?;
        store.clear_staging(&build_id).await?;
        let error = store
            .renew_staging(&build_id)
            .await
            .expect_err("a reclaimed staging marker must stop the builder");
        assert!(error.to_string().contains("staging lease disappeared"));
        Ok(())
    }

    #[tokio::test]
    async fn markerless_old_orphan_build_is_collectable() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
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
        assert!(
            !operator
                .exists(&store.garbage_marker_path("orphan"))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn gc_does_not_trust_a_stale_head_hint_after_head_removal() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA012);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let orphan = format!(
            "{}/metadata/orphan.json",
            store.builds_path("stale-head-hint")
        );
        operator.write(&orphan, b"orphan".to_vec()).await?;

        assert!(
            store
                .has_pending_garbage(Some("stale-head-hint"), Duration::ZERO)
                .await?
        );
        assert_eq!(
            store
                .garbage_collect(Some("stale-head-hint"), Duration::ZERO)
                .await?,
            vec!["stale-head-hint"]
        );
        assert!(!operator.exists(&orphan).await?);
        Ok(())
    }

    #[test]
    fn uuid_v7_build_timestamp_is_a_durable_gc_age_fallback() {
        let old_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_millis()
            .saturating_sub(Duration::from_secs(2 * 60 * 60).as_millis())
            as u64;
        let mut bytes = [0_u8; 16];
        bytes[..6].copy_from_slice(&old_millis.to_be_bytes()[2..]);
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        let old_build_id = Uuid::from_bytes(bytes).to_string();

        // This is the path used when a backend returns no last_modified for
        // a markerless orphan. The persisted UUIDv7 timestamp still enforces
        // a non-zero grace period instead of deleting a fresh build.
        assert!(DerivedRelationHeadStore::old_enough(
            DerivedRelationHeadStore::build_id_time(&old_build_id),
            Duration::from_secs(60 * 60),
        ));
        let fresh_build_id = Uuid::now_v7().to_string();
        assert!(!DerivedRelationHeadStore::old_enough(
            DerivedRelationHeadStore::build_id_time(&fresh_build_id),
            Duration::from_secs(60 * 60),
        ));
        assert!(DerivedRelationHeadStore::build_id_time(&Uuid::new_v4().to_string()).is_none());
        bytes[8] = 0;
        assert!(
            DerivedRelationHeadStore::build_id_time(&Uuid::from_bytes(bytes).to_string()).is_none()
        );
    }

    #[test]
    fn terminal_claim_retention_has_a_bounded_clock_window() {
        let claimed_at = UNIX_EPOCH + Duration::from_secs(100);
        let retention = Duration::from_secs(7 * 24 * 60 * 60);
        assert!(!DerivedRelationHeadStore::old_enough_at(
            Some(claimed_at),
            retention,
            claimed_at + retention - Duration::from_secs(1),
        ));
        assert!(DerivedRelationHeadStore::old_enough_at(
            Some(claimed_at),
            retention,
            claimed_at + retention,
        ));
    }

    #[tokio::test]
    async fn terminal_tombstone_reaping_is_bounded_per_pass() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA015);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        for _ in 0..=MAX_DERIVED_TERMINAL_TOMBSTONES_PER_PASS {
            let build_id = Uuid::now_v7().to_string();
            operator
                .write(
                    &store.terminal_tombstone_path(&build_id),
                    serde_json::to_vec(&serde_json::json!({
                        "build_id": build_id,
                        "state": "complete",
                    }))?,
                )
                .await?;
        }

        store
            .reap_expired_terminal_tombstones_at_with_retention(SystemTime::now(), Duration::ZERO)
            .await?;

        let lister = operator
            .lister_with(&store.terminal_tombstones_prefix())
            .recursive(true)
            .await?;
        let remaining = lister.try_collect::<Vec<_>>().await?.len();
        assert_eq!(remaining, 1);
        Ok(())
    }

    #[tokio::test]
    async fn terminal_tombstone_build_id_cannot_be_staged_again() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA013);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let reaped_build_id = Uuid::now_v7().to_string();
        operator
            .write(
                &store.terminal_tombstone_path(&reaped_build_id),
                br#"{"build_id":"terminal","state":"complete"}"#.to_vec(),
            )
            .await?;

        let error = store
            .mark_staging(&reaped_build_id)
            .await
            .expect_err("a terminally reaped UUIDv7 ID must stay fenced");
        assert!(error.to_string().contains("terminal tombstone"));
        assert!(
            !operator
                .exists(&store.staging_marker_path(&reaped_build_id))
                .await?
        );

        let fresh_build_id = Uuid::now_v7().to_string();
        store.mark_staging(&fresh_build_id).await?;
        assert!(
            operator
                .exists(&store.staging_marker_path(&fresh_build_id))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn non_uuid_v7_build_id_is_rejected_before_staging() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let store =
            DerivedRelationHeadStore::new(operator, "spaces/demo", uuid::Uuid::from_u128(0xA014));
        let error = store
            .mark_staging("legacy-build")
            .await
            .expect_err("non-UUIDv7 build IDs must not enter the lifecycle");
        assert!(error.to_string().contains("must be UUIDv7"));
        Ok(())
    }

    #[tokio::test]
    async fn gc_does_not_block_on_active_legacy_head() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA010);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        operator
            .write(
                &store.head_path(),
                serde_json::to_vec(&serde_json::json!({
                    "format_version": 1,
                    "materialization_id": "legacy-materialization",
                    "generation": 1
                }))?,
            )
            .await?;
        let orphan = format!("{}/data/orphan.parquet", store.builds_path("orphan"));
        operator.write(&orphan, b"orphan".to_vec()).await?;

        assert_eq!(
            store.garbage_collect(None, Duration::ZERO).await?,
            vec!["orphan"]
        );
        assert!(!operator.exists(&orphan).await?);
        assert!(operator.exists(&store.head_path()).await?);
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_single_process_create_has_one_winner() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA002);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id)
            .single_process();
        let space_uid = Uuid::now_v7();
        write_test_space_metadata(&operator, space_uid).await?;
        let build_id = test_build_id();
        let head = DerivedRelationHead {
            format_version: 1,
            space_id: space_uid.to_string(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            build_id: build_id.clone(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: Uuid::now_v7().to_string(),
            metadata_location: format!(
                "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{build_id}/metadata.json"
            ),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            head_fence: String::new(),
            checksum: String::new(),
        };
        store.mark_staging(&head.build_id).await?;
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
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA003);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id)
            .single_process();
        let space_uid = Uuid::now_v7();
        write_test_space_metadata(&operator, space_uid).await?;
        let current_build_id = test_build_id();
        operator
            .write(
                &format!("{}/manifest.json", store.builds_path(&current_build_id)),
                b"current".to_vec(),
            )
            .await?;
        operator
            .write(
                &format!("{}/manifest.json", store.builds_path("stale")),
                b"stale".to_vec(),
            )
            .await?;
        let current_head = DerivedRelationHead {
            format_version: 1,
            space_id: space_uid.to_string(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            build_id: current_build_id.clone(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: Uuid::now_v7().to_string(),
            metadata_location: format!(
                "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{current_build_id}/metadata.json"
            ),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            head_fence: String::new(),
            checksum: String::new(),
        };
        store.mark_staging(&current_build_id).await?;
        store.create(&current_head).await?;
        store.mark_garbage("stale").await?;
        let deleted = store
            .garbage_collect(Some("current"), Duration::ZERO)
            .await?;
        assert_eq!(deleted, vec!["stale"]);
        assert!(
            operator
                .exists(&format!(
                    "{}/manifest.json",
                    store.builds_path(&current_build_id)
                ))
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
        let operator = Operator::new(Memory::default())?;
        let relation_id = uuid::Uuid::from_u128(0xA004);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id);
        let space_uid = Uuid::now_v7();
        write_test_space_metadata(&operator, space_uid).await?;
        let first_build_id = test_build_id();
        let mut first = DerivedRelationHead {
            format_version: 1,
            space_id: space_uid.to_string(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer".into(),
            compatibility_epoch: 1,
            build_id: first_build_id.clone(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: Uuid::now_v7().to_string(),
            metadata_location: format!(
                "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{first_build_id}/metadata.json"
            ),
            snapshot_id: None,
            schema_id: 0,
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            head_fence: String::new(),
            checksum: String::new(),
        };
        store.mark_staging(&first.build_id).await?;
        store.publish(None, &first).await?;
        let claim: serde_json::Value = serde_json::from_slice(
            &operator
                .read(&store.publishing_marker_path(&first.build_id))
                .await?
                .to_vec(),
        )?;
        assert_eq!(claim["role"], "released");
        let expected = store.read_exact().await?.expect("initial Head");
        first.generation = 2;
        first.build_id = test_build_id();
        first.metadata_location = format!(
            "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{}/metadata.json",
            first.build_id
        );
        store.mark_staging(&first.build_id).await?;
        store.publish(Some(&expected), &first).await?;
        let mut loser = first.clone();
        loser.generation = 3;
        loser.build_id = test_build_id();
        loser.metadata_location = format!(
            "memory:///spaces/demo/_ugoite/derived/relations/{relation_id}/builds/{}/metadata.json",
            loser.build_id
        );
        store.mark_staging(&loser.build_id).await?;
        let error = store
            .publish(Some(&expected), &loser)
            .await
            .expect_err("stale writer must lose CAS");
        assert!(error.to_string().contains("changed"));
        assert_eq!(
            store.read_exact().await?.unwrap().head.build_id,
            first.build_id
        );
        let loser_claim: serde_json::Value = serde_json::from_slice(
            &operator
                .read(&store.publishing_marker_path(&loser.build_id))
                .await?
                .to_vec(),
        )?;
        assert_eq!(loser_claim["role"], "released");
        Ok(())
    }

    #[tokio::test]
    async fn single_process_relation_lock_serializes_full_rebuilds() -> Result<()> {
        let operator = Operator::new(Memory::default())?;
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
