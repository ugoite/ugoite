//! Iceberg storage backed by one operator-bound Ugoite Space.
//!
//! Iceberg keeps object coordinates inside table metadata and manifest files.
//! Those coordinates must be portable, so this adapter accepts only
//! `ugoite://{space_uid}/{space-relative-key}` and resolves the key against
//! the operator that was explicitly bound to the Space.

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::StreamExt;
use iceberg::io::{
    FileMetadata, FileRead, FileWrite, InputFile, OutputFile, Storage, StorageFactory,
};
use iceberg::{Error, ErrorKind, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::sync::Arc;
use ugoite_domain::id::SpaceId;
use ugoite_domain::space_key::{SpaceKey, SpaceUri};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LogicalStorageFactory {
    #[serde(skip, default = "default_operator")]
    operator: Operator,
    space_root: String,
    space_uid: Uuid,
}

fn default_operator() -> Operator {
    Operator::new(opendal::services::Memory::default()).expect("memory operator configuration")
}

impl LogicalStorageFactory {
    pub(crate) fn new(operator: Operator, space_root: impl Into<String>, space_uid: Uuid) -> Self {
        Self {
            operator,
            space_root: space_root.into(),
            space_uid,
        }
    }
}

#[typetag::serde(name = "UgoiteLogicalStorageFactory")]
impl StorageFactory for LogicalStorageFactory {
    fn build(&self, _config: &iceberg::io::StorageConfig) -> Result<Arc<dyn Storage>> {
        Ok(Arc::new(LogicalStorage {
            operator: self.operator.clone(),
            space_root: self.space_root.clone(),
            space_uid: self.space_uid,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogicalStorage {
    #[serde(skip, default = "default_operator")]
    operator: Operator,
    space_root: String,
    space_uid: Uuid,
}

impl LogicalStorage {
    fn resolve(&self, location: &str) -> Result<String> {
        let uri = SpaceUri::parse(location).map_err(|error| {
            Error::new(
                ErrorKind::DataInvalid,
                format!("invalid Ugoite Iceberg logical URI: {location}"),
            )
            .with_source(error)
        })?;
        if uri.space_uid() != self.space_uid {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Ugoite Iceberg logical URI belongs to another Space",
            ));
        }
        Ok(format!(
            "{}/{}",
            self.space_root.trim_end_matches('/'),
            uri.key()
        ))
    }

    fn map_error(operation: &str, location: &str, error: opendal::Error) -> Error {
        let kind = match error.kind() {
            opendal::ErrorKind::NotFound => ErrorKind::DataInvalid,
            _ => ErrorKind::Unexpected,
        };
        Error::new(kind, format!("Iceberg {operation} failed for {location}")).with_source(error)
    }
}

struct LogicalReader {
    reader: opendal::Reader,
}

#[async_trait]
impl FileRead for LogicalReader {
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        self.reader
            .read(range)
            .await
            .map(|bytes| bytes.to_bytes())
            .map_err(|error| {
                Error::new(ErrorKind::Unexpected, "Iceberg logical object read failed")
                    .with_source(error)
            })
    }
}

struct LogicalWriter {
    writer: opendal::Writer,
}

#[async_trait]
impl FileWrite for LogicalWriter {
    async fn write(&mut self, bytes: Bytes) -> Result<()> {
        self.writer.write(bytes).await.map_err(|error| {
            Error::new(ErrorKind::Unexpected, "Iceberg logical object write failed")
                .with_source(error)
        })
    }

    async fn close(&mut self) -> Result<()> {
        self.writer.close().await.map(|_| ()).map_err(|error| {
            Error::new(ErrorKind::Unexpected, "Iceberg logical object close failed")
                .with_source(error)
        })
    }
}

#[async_trait]
#[typetag::serde(name = "UgoiteLogicalStorage")]
impl Storage for LogicalStorage {
    async fn exists(&self, location: &str) -> Result<bool> {
        let path = self.resolve(location)?;
        self.operator
            .exists(&path)
            .await
            .map_err(|error| Self::map_error("exists", location, error))
    }

    async fn metadata(&self, location: &str) -> Result<FileMetadata> {
        let path = self.resolve(location)?;
        self.operator
            .stat(&path)
            .await
            .map(|metadata| FileMetadata {
                size: metadata.content_length(),
            })
            .map_err(|error| Self::map_error("metadata read", location, error))
    }

    async fn read(&self, location: &str) -> Result<Bytes> {
        let path = self.resolve(location)?;
        self.operator
            .read(&path)
            .await
            .map(|bytes| bytes.to_bytes())
            .map_err(|error| Self::map_error("read", location, error))
    }

    async fn reader(&self, location: &str) -> Result<Box<dyn FileRead>> {
        let path = self.resolve(location)?;
        self.operator
            .reader(&path)
            .await
            .map(|reader| Box::new(LogicalReader { reader }) as Box<dyn FileRead>)
            .map_err(|error| Self::map_error("reader open", location, error))
    }

    async fn write(&self, location: &str, bytes: Bytes) -> Result<()> {
        let path = self.resolve(location)?;
        self.operator
            .write(&path, bytes.to_vec())
            .await
            .map(|_| ())
            .map_err(|error| Self::map_error("write", location, error))
    }

    async fn writer(&self, location: &str) -> Result<Box<dyn FileWrite>> {
        let path = self.resolve(location)?;
        self.operator
            .writer(&path)
            .await
            .map(|writer| Box::new(LogicalWriter { writer }) as Box<dyn FileWrite>)
            .map_err(|error| Self::map_error("writer open", location, error))
    }

    async fn delete(&self, location: &str) -> Result<()> {
        let path = self.resolve(location)?;
        self.operator
            .delete(&path)
            .await
            .map_err(|error| Self::map_error("delete", location, error))
    }

    async fn delete_prefix(&self, location: &str) -> Result<()> {
        let path = self.resolve(location)?;
        self.operator
            .delete_with(&path)
            .recursive(true)
            .await
            .map_err(|error| Self::map_error("prefix delete", location, error))
    }

    async fn delete_stream(&self, mut locations: BoxStream<'static, String>) -> Result<()> {
        while let Some(location) = locations.next().await {
            self.delete(&location).await?;
        }
        Ok(())
    }

    fn new_input(&self, location: &str) -> Result<InputFile> {
        self.resolve(location)?;
        Ok(InputFile::new(Arc::new(self.clone()), location.to_owned()))
    }

    fn new_output(&self, location: &str) -> Result<OutputFile> {
        self.resolve(location)?;
        Ok(OutputFile::new(Arc::new(self.clone()), location.to_owned()))
    }
}

/// Returns the logical identity used by a Space's Iceberg coordinates.
///
/// Production Space metadata always carries UUIDv7. The deterministic fallback
/// exists only for the crate's in-memory fixtures, which intentionally use
/// compact synthetic IDs for namespaces and Catalog Head tests.
pub(crate) fn logical_space_uid(space_id: SpaceId) -> Uuid {
    let uid = space_id.as_uuid();
    if uid.get_version() == Some(uuid::Version::SortRand) {
        return uid;
    }
    let mut bytes = *Uuid::new_v5(&Uuid::NAMESPACE_OID, uid.as_bytes()).as_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub(crate) fn logical_uri(space_uid: Uuid, key: &str) -> Result<String> {
    let key = SpaceKey::parse(key).map_err(|error| {
        Error::new(
            ErrorKind::DataInvalid,
            format!("invalid logical Space key: {key}"),
        )
        .with_source(error)
    })?;
    SpaceUri::new(space_uid, key)
        .map(|uri| uri.to_string())
        .map_err(|error| {
            Error::new(ErrorKind::DataInvalid, "invalid logical Space identity").with_source(error)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result as AnyResult;
    use iceberg::io::FileIOBuilder;
    use opendal::services::{Fs, Memory};
    use opendal::Operator;
    use tempfile::tempdir;

    #[tokio::test]
    async fn logical_coordinates_resolve_to_the_bound_memory_space() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?;
        let space_uid = Uuid::now_v7();
        let file_io = FileIOBuilder::new(Arc::new(LogicalStorageFactory::new(
            operator.clone(),
            "spaces/demo",
            space_uid,
        )))
        .build();
        let location = logical_uri(space_uid, "forms/form/metadata.json")?;

        file_io
            .new_output(&location)?
            .write(Bytes::from_static(b"metadata"))
            .await?;

        assert!(
            operator
                .exists("spaces/demo/forms/form/metadata.json")
                .await?
        );
        assert!(file_io
            .exists(&logical_uri(Uuid::now_v7(), "forms/form/metadata.json")?)
            .await
            .is_err());
        assert!(file_io
            .exists("memory:///spaces/demo/forms/form/metadata.json")
            .await
            .is_err());
        assert!(file_io
            .exists(&format!("ugoite://{space_uid}/forms/../metadata.json"))
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn the_same_logical_coordinate_resolves_on_a_local_space() -> AnyResult<()> {
        let directory = tempdir()?;
        let operator =
            Operator::new(Fs::default().root(directory.path().to_string_lossy().as_ref()))?;
        let space_uid = Uuid::now_v7();
        let file_io = FileIOBuilder::new(Arc::new(LogicalStorageFactory::new(
            operator,
            "spaces/demo",
            space_uid,
        )))
        .build();
        let location = logical_uri(space_uid, "forms/form/data.parquet")?;

        file_io
            .new_output(&location)?
            .write(Bytes::from_static(b"data"))
            .await?;

        assert!(directory
            .path()
            .join("spaces/demo/forms/form/data.parquet")
            .is_file());
        Ok(())
    }

    #[test]
    fn synthetic_test_space_ids_get_a_stable_uuidv7_coordinate_identity() {
        let space_id = SpaceId::from(Uuid::from_u128(1));
        let first = logical_space_uid(space_id);
        let second = logical_space_uid(space_id);
        assert_eq!(first, second);
        assert_eq!(first.get_version(), Some(uuid::Version::SortRand));
    }
}
