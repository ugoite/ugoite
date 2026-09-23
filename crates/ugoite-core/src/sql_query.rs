//! Stateless read-only SQL query contracts.
//!
//! SQL execution state is deliberately client-held.  A continuation records
//! only the immutable publication and query coordinate needed to repeat a
//! bounded page; it is never an authorization credential.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use ugoite_domain::id::SpaceId;
use ugoite_domain::publication_ref::PublicationRef;

pub const SQL_CONTINUATION_VERSION: u32 = 1;
pub const MAX_SQL_QUERY_BYTES: usize = 256 * 1024;
pub const MAX_SQL_PARAMETER_BYTES: usize = 256 * 1024;
pub const MAX_SQL_PAGE_LIMIT: usize = 1_000;
pub const MAX_SQL_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SQL_OUTPUT_COLUMNS: usize = 256;
pub const MAX_SQL_COLUMN_NAME_BYTES: usize = 16 * 1024;
pub const MAX_SQL_COLUMN_METADATA_BYTES: usize = 1024 * 1024;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlQueryRequest {
    pub sql: String,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub parameter_types: BTreeMap<String, String>,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
}

impl SqlQueryRequest {
    pub fn validate(&self) -> Result<(), SqlQueryError> {
        if self.sql.trim().is_empty() || self.sql.len() > MAX_SQL_QUERY_BYTES {
            return Err(SqlQueryError::Invalid(
                "SQL query must be non-empty and within the configured byte limit".to_string(),
            ));
        }
        if self.limit == 0 || self.limit > MAX_SQL_PAGE_LIMIT {
            return Err(SqlQueryError::Invalid(
                "SQL query page limit is out of range".to_string(),
            ));
        }
        let parameter_bytes = serde_json::to_vec(&(&self.parameters, &self.parameter_types))
            .map_err(SqlQueryError::Serialize)?
            .len();
        if parameter_bytes > MAX_SQL_PARAMETER_BYTES {
            return Err(SqlQueryError::Invalid(
                "SQL query parameters exceed the configured byte limit".to_string(),
            ));
        }
        Ok(())
    }

    pub fn sql_fingerprint(&self, normalized_sql: &str) -> Result<String, SqlQueryError> {
        fingerprint(normalized_sql.as_bytes())
    }

    pub fn parameter_fingerprint(&self) -> Result<String, SqlQueryError> {
        let canonical = (&self.parameters, &self.parameter_types);
        let bytes = serde_json::to_vec(&canonical).map_err(SqlQueryError::Serialize)?;
        fingerprint(&bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlQueryCountRequest {
    pub sql: String,
    #[serde(default)]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub parameter_types: BTreeMap<String, String>,
}

impl SqlQueryCountRequest {
    pub fn validate(&self) -> Result<(), SqlQueryError> {
        SqlQueryRequest {
            sql: self.sql.clone(),
            parameters: self.parameters.clone(),
            parameter_types: self.parameter_types.clone(),
            limit: 1,
            continuation: None,
        }
        .validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlQueryPage {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlContinuation {
    pub version: u32,
    pub space_id: SpaceId,
    pub publication: PublicationRef,
    pub sql_fingerprint: String,
    pub parameter_fingerprint: String,
    pub authorization_fingerprint: String,
    pub offset: usize,
}

impl SqlContinuation {
    pub fn new(
        space_id: SpaceId,
        publication: PublicationRef,
        sql_fingerprint: String,
        parameter_fingerprint: String,
        authorization_fingerprint: String,
        offset: usize,
    ) -> Result<Self, SqlQueryError> {
        publication
            .validate()
            .map_err(|error| SqlQueryError::Invalid(error.to_string()))?;
        if sql_fingerprint.is_empty()
            || parameter_fingerprint.is_empty()
            || authorization_fingerprint.is_empty()
        {
            return Err(SqlQueryError::Invalid(
                "SQL continuation fingerprints must not be empty".to_string(),
            ));
        }
        Ok(Self {
            version: SQL_CONTINUATION_VERSION,
            space_id,
            publication,
            sql_fingerprint,
            parameter_fingerprint,
            authorization_fingerprint,
            offset,
        })
    }

    pub fn encode(&self, signing_key: &[u8]) -> Result<String, SqlQueryError> {
        if signing_key.is_empty() {
            return Err(SqlQueryError::Invalid(
                "SQL continuation signing key must not be empty".to_string(),
            ));
        }
        let payload = serde_json::to_vec(self).map_err(SqlQueryError::Serialize)?;
        let mut mac = HmacSha256::new_from_slice(signing_key)
            .map_err(|_| SqlQueryError::Invalid("invalid SQL continuation key".to_string()))?;
        mac.update(&payload);
        Ok(format!(
            "v{}.{}.{}",
            SQL_CONTINUATION_VERSION,
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        ))
    }

    pub fn decode(token: &str, signing_key: &[u8]) -> Result<Self, SqlQueryError> {
        let mut parts = token.split('.');
        let version = parts
            .next()
            .ok_or_else(|| SqlQueryError::Invalid("SQL continuation is malformed".to_string()))?;
        let payload = parts
            .next()
            .ok_or_else(|| SqlQueryError::Invalid("SQL continuation is malformed".to_string()))?;
        let signature = parts
            .next()
            .ok_or_else(|| SqlQueryError::Invalid("SQL continuation is malformed".to_string()))?;
        if parts.next().is_some() || version != "v1" {
            return Err(SqlQueryError::Invalid(
                "SQL continuation version is unsupported".to_string(),
            ));
        }
        if signing_key.is_empty() {
            return Err(SqlQueryError::Invalid(
                "SQL continuation signing key must not be empty".to_string(),
            ));
        }
        let payload_bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| {
            SqlQueryError::Invalid("SQL continuation payload is malformed".to_string())
        })?;
        let signature_bytes = URL_SAFE_NO_PAD.decode(signature).map_err(|_| {
            SqlQueryError::Invalid("SQL continuation signature is malformed".to_string())
        })?;
        let mut mac = HmacSha256::new_from_slice(signing_key)
            .map_err(|_| SqlQueryError::Invalid("invalid SQL continuation key".to_string()))?;
        mac.update(&payload_bytes);
        mac.verify_slice(&signature_bytes)
            .map_err(|_| SqlQueryError::Tampered)?;
        let continuation: Self =
            serde_json::from_slice(&payload_bytes).map_err(SqlQueryError::Deserialize)?;
        if continuation.version != SQL_CONTINUATION_VERSION {
            return Err(SqlQueryError::Invalid(
                "SQL continuation version is unsupported".to_string(),
            ));
        }
        continuation
            .publication
            .validate()
            .map_err(|error| SqlQueryError::Invalid(error.to_string()))?;
        Ok(continuation)
    }

    pub fn authorize(&self, current_fingerprint: &str) -> Result<(), SqlQueryError> {
        if self.authorization_fingerprint == current_fingerprint {
            Ok(())
        } else {
            Err(SqlQueryError::AuthorizationChanged)
        }
    }
}

fn fingerprint(bytes: &[u8]) -> Result<String, SqlQueryError> {
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[derive(Debug)]
pub enum SqlQueryError {
    Invalid(String),
    Tampered,
    AuthorizationChanged,
    Serialize(serde_json::Error),
    Deserialize(serde_json::Error),
}

impl fmt::Display for SqlQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::Tampered => formatter.write_str("SQL continuation signature is invalid"),
            Self::AuthorizationChanged => {
                formatter.write_str("authorization changed; restart the SQL query")
            }
            Self::Serialize(error) => write!(formatter, "serialize SQL query contract: {error}"),
            Self::Deserialize(error) => write!(formatter, "decode SQL continuation: {error}"),
        }
    }
}

impl std::error::Error for SqlQueryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::space_key::SpaceUri;
    use uuid::Uuid;

    fn publication() -> PublicationRef {
        PublicationRef::new(
            1,
            SpaceUri::parse("ugoite://018f6c7e-5f6a-7b8c-9d0e-1f2a3b4c5d6e/publications/1")
                .expect("space uri"),
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .expect("publication")
    }

    #[test]
    fn continuation_round_trips_and_rejects_tampering() {
        let continuation = SqlContinuation::new(
            Uuid::parse_str("01900000-0000-7000-8000-000000000001")
                .expect("space id")
                .into(),
            publication(),
            "sql".repeat(16),
            "parameter".repeat(8),
            "auth".repeat(16),
            100,
        )
        .expect("continuation");
        let encoded = continuation.encode(b"secret").expect("encode");
        assert_eq!(
            SqlContinuation::decode(&encoded, b"secret").expect("decode"),
            continuation
        );
        let mut tampered = encoded.clone();
        tampered.push('x');
        assert!(matches!(
            SqlContinuation::decode(&tampered, b"secret"),
            Err(SqlQueryError::Tampered | SqlQueryError::Invalid(_))
        ));
        assert!(matches!(
            continuation.authorize("different"),
            Err(SqlQueryError::AuthorizationChanged)
        ));
    }
}
