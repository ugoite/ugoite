//! Portable references to immutable Space publications.
//!
//! A reference contains only the coordinates needed to resolve one immutable
//! publication. Space identity is already carried by `SpaceUri`; it is not
//! duplicated here.

use crate::space_key::SpaceUri;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublicationRef {
    pub generation: u64,
    pub publication_uri: SpaceUri,
    pub publication_checksum: String,
}

impl PublicationRef {
    pub fn new(
        generation: u64,
        publication_uri: SpaceUri,
        publication_checksum: impl Into<String>,
    ) -> Result<Self, PublicationRefError> {
        let publication_checksum = publication_checksum.into();
        if !is_sha256_hex(&publication_checksum) {
            return Err(if publication_checksum.is_empty() {
                PublicationRefError::MissingChecksum
            } else {
                PublicationRefError::InvalidChecksum
            });
        }
        Ok(Self {
            generation,
            publication_uri,
            publication_checksum,
        })
    }

    pub fn validate(&self) -> Result<(), PublicationRefError> {
        if !is_sha256_hex(&self.publication_checksum) {
            return Err(if self.publication_checksum.is_empty() {
                PublicationRefError::MissingChecksum
            } else {
                PublicationRefError::InvalidChecksum
            });
        }
        Ok(())
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PublicationRefError {
    MissingChecksum,
    InvalidChecksum,
}

impl std::fmt::Display for PublicationRefError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingChecksum => formatter.write_str("publication checksum must not be empty"),
            Self::InvalidChecksum => {
                formatter.write_str("publication checksum must be a lowercase SHA-256 digest")
            }
        }
    }
}

impl std::error::Error for PublicationRefError {}
