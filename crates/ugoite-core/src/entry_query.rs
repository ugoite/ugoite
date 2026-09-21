//! Canonical logical Entry query semantics.
//!
//! This module intentionally contains no storage-engine, SQL, or transport
//! implementation.  Every human and machine Entry reader uses these logical
//! types; adapters resolve the logical field references inside their own
//! authorized execution boundary.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId, SpaceId};
use ugoite_domain::publication_ref::PublicationRef;

use crate::structured_search::SearchOperator;

pub const ENTRY_CURSOR_VERSION: u32 = 1;
pub const MAX_ENTRY_QUERY_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_ENTRY_FILTERS: usize = 32;
pub const MAX_ENTRY_SORTS: usize = 8;
pub const MAX_ENTRY_PROJECTION_FIELDS: usize = 64;
pub const MAX_ENTRY_PAGE_LIMIT: usize = 1_000;

type HmacSha256 = Hmac<Sha256>;

/// The semantic scope of an Entry query.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryQueryScope {
    #[default]
    All,
    Form {
        form_id: FormId,
    },
}

/// A logical Entry field. Property fields are meaningful only inside a Form
/// scope; system fields remain valid for All Forms queries.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryFieldRef {
    Property { field_id: FieldId },
    Form,
    CreatedAt,
    UpdatedAt,
}

impl EntryFieldRef {
    pub const fn is_property(self) -> bool {
        matches!(self, Self::Property { .. })
    }
}

/// One canonical logical filter. `field` is never a SQL relation or column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFilter {
    pub field: EntryFieldRef,
    pub operator: SearchOperator,
    pub value: Value,
}

/// One user-provided sort clause. Stable hidden tie-breakers are added by the
/// storage compiler and are deliberately not represented here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrySort {
    pub field: EntryFieldRef,
    pub direction: EntrySortDirection,
}

/// Logical query identity. Projection and pagination are intentionally absent
/// so a caller can change visible columns without invalidating the query chain.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryQuery {
    pub scope: EntryQueryScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<EntryFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<EntrySort>,
}

impl EntryQuery {
    pub fn validate(&self) -> Result<(), EntryQueryError> {
        if self
            .text
            .as_deref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > MAX_ENTRY_QUERY_TEXT_BYTES)
        {
            return Err(EntryQueryError::Invalid(
                "Entry query text must be non-empty and within the configured byte limit"
                    .to_string(),
            ));
        }
        if self.filters.len() > MAX_ENTRY_FILTERS {
            return Err(EntryQueryError::Invalid(
                "Entry query exceeds the maximum filter count".to_string(),
            ));
        }
        if self.sort.len() > MAX_ENTRY_SORTS {
            return Err(EntryQueryError::Invalid(
                "Entry query exceeds the maximum sort count".to_string(),
            ));
        }
        for field in self
            .filters
            .iter()
            .map(|filter| filter.field)
            .chain(self.sort.iter().map(|sort| sort.field))
        {
            validate_field_for_scope(field, &self.scope)?;
        }
        Ok(())
    }

    /// Returns the stable query fingerprint. Projection is excluded because
    /// it is not part of `EntryQuery` and must not invalidate a cursor.
    pub fn fingerprint(&self) -> Result<String, EntryQueryError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(EntryQueryError::Serialize)?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

/// Requested visible columns. The compiler may always include hidden control
/// columns needed to produce the stable result identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryProjection {
    Fields { fields: Vec<EntryFieldRef> },
    Preview,
}

impl EntryProjection {
    pub fn validate(&self) -> Result<(), EntryQueryError> {
        if let Self::Fields { fields } = self {
            if fields.is_empty() || fields.len() > MAX_ENTRY_PROJECTION_FIELDS {
                return Err(EntryQueryError::Invalid(
                    "Entry projection field count is out of range".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn validate_for_scope(&self, scope: &EntryQueryScope) -> Result<(), EntryQueryError> {
        self.validate()?;
        if let Self::Fields { fields } = self {
            for field in fields {
                validate_field_for_scope(*field, scope)?;
            }
        }
        Ok(())
    }
}

fn validate_field_for_scope(
    field: EntryFieldRef,
    scope: &EntryQueryScope,
) -> Result<(), EntryQueryError> {
    match (field, scope) {
        (EntryFieldRef::Property { .. }, EntryQueryScope::All) => {
            Err(EntryQueryError::PropertyRequiresFormScope)
        }
        (EntryFieldRef::Form, EntryQueryScope::Form { .. }) => Err(EntryQueryError::Invalid(
            "Form identity requires an All Forms Entry query".to_string(),
        )),
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPageRequest {
    pub query: EntryQuery,
    pub projection: EntryProjection,
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

impl EntryPageRequest {
    pub fn validate(&self) -> Result<(), EntryQueryError> {
        self.query.validate()?;
        self.projection.validate_for_scope(&self.query.scope)?;
        if self.limit == 0 || self.limit > MAX_ENTRY_PAGE_LIMIT {
            return Err(EntryQueryError::Invalid(
                "Entry query page limit is out of range".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryCountRequest {
    pub query: EntryQuery,
}

impl EntryCountRequest {
    pub fn validate(&self) -> Result<(), EntryQueryError> {
        self.query.validate()
    }
}

/// The semantic result row. `properties` and `preview` are projection-owned;
/// identity and revision control data remain machine-readable and stable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryResult {
    pub id: EntryId,
    pub form_id: FormId,
    pub revision_id: RevisionId,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPage {
    pub rows: Vec<EntryResult>,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntryCount {
    pub count: u64,
}

/// Query capability derived from a Form definition. Frontends and CLIs render
/// this descriptor; they do not maintain a second operator/type truth table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFieldCapability {
    pub field: EntryFieldRef,
    pub name: String,
    pub field_type: String,
    pub filterable: bool,
    pub sortable: bool,
    pub projectable: bool,
    pub supported_operators: Vec<SearchOperator>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryQueryCapabilities {
    pub scope: EntryQueryScope,
    pub fields: Vec<EntryFieldCapability>,
}

/// Opaque, signed keyset continuation. The token carries its immutable
/// publication coordinate and never acts as an authorization credential.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryCursor {
    pub version: u32,
    pub space_id: SpaceId,
    pub publication: PublicationRef,
    pub query_fingerprint: String,
    pub authorization_fingerprint: String,
    pub sort_values: Vec<Value>,
    pub form_id: Option<FormId>,
    pub entry_id: EntryId,
}

impl EntryCursor {
    pub fn new(
        space_id: SpaceId,
        publication: PublicationRef,
        query_fingerprint: String,
        authorization_fingerprint: String,
        sort_values: Vec<Value>,
        form_id: Option<FormId>,
        entry_id: EntryId,
    ) -> Result<Self, EntryCursorError> {
        publication
            .validate()
            .map_err(|error| EntryCursorError::Invalid(error.to_string()))?;
        if query_fingerprint.is_empty() || authorization_fingerprint.is_empty() {
            return Err(EntryCursorError::Invalid(
                "cursor fingerprints must not be empty".to_string(),
            ));
        }
        if sort_values.is_empty() {
            return Err(EntryCursorError::Invalid(
                "cursor sort tuple must not be empty".to_string(),
            ));
        }
        Ok(Self {
            version: ENTRY_CURSOR_VERSION,
            space_id,
            publication,
            query_fingerprint,
            authorization_fingerprint,
            sort_values,
            form_id,
            entry_id,
        })
    }

    pub fn encode(&self, signing_key: &[u8]) -> Result<String, EntryCursorError> {
        if signing_key.is_empty() {
            return Err(EntryCursorError::Invalid(
                "cursor signing key must not be empty".to_string(),
            ));
        }
        let payload = serde_json::to_vec(self).map_err(EntryCursorError::Serialize)?;
        let mut mac = HmacSha256::new_from_slice(signing_key)
            .map_err(|_| EntryCursorError::Invalid("invalid cursor signing key".to_string()))?;
        mac.update(&payload);
        Ok(format!(
            "v{}.{}.{}",
            ENTRY_CURSOR_VERSION,
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        ))
    }

    pub fn decode(token: &str, signing_key: &[u8]) -> Result<Self, EntryCursorError> {
        let mut parts = token.split('.');
        let version = parts
            .next()
            .ok_or_else(|| EntryCursorError::Invalid("cursor is malformed".to_string()))?;
        let payload = parts
            .next()
            .ok_or_else(|| EntryCursorError::Invalid("cursor is malformed".to_string()))?;
        let signature = parts
            .next()
            .ok_or_else(|| EntryCursorError::Invalid("cursor is malformed".to_string()))?;
        if parts.next().is_some() || version != "v1" {
            return Err(EntryCursorError::Invalid(
                "cursor version is unsupported".to_string(),
            ));
        }
        if signing_key.is_empty() {
            return Err(EntryCursorError::Invalid(
                "cursor signing key must not be empty".to_string(),
            ));
        }
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| EntryCursorError::Invalid("cursor payload is malformed".to_string()))?;
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| EntryCursorError::Invalid("cursor signature is malformed".to_string()))?;
        let mut mac = HmacSha256::new_from_slice(signing_key)
            .map_err(|_| EntryCursorError::Invalid("invalid cursor signing key".to_string()))?;
        mac.update(&payload_bytes);
        mac.verify_slice(&signature_bytes)
            .map_err(|_| EntryCursorError::Tampered)?;
        let cursor: Self =
            serde_json::from_slice(&payload_bytes).map_err(EntryCursorError::Deserialize)?;
        if cursor.version != ENTRY_CURSOR_VERSION {
            return Err(EntryCursorError::Invalid(
                "cursor version is unsupported".to_string(),
            ));
        }
        cursor.publication.validate().map_err(|error| {
            EntryCursorError::Invalid(format!("cursor publication is invalid: {error}"))
        })?;
        if cursor.query_fingerprint.is_empty()
            || cursor.authorization_fingerprint.is_empty()
            || cursor.sort_values.is_empty()
        {
            return Err(EntryCursorError::Invalid(
                "cursor payload is incomplete".to_string(),
            ));
        }
        Ok(cursor)
    }

    /// Adapters resolve `publication` to their immutable checkpoint before
    /// executing a continuation.
    pub fn publication_identity(&self) -> &PublicationRef {
        &self.publication
    }

    /// A cursor is intentionally not an authorization token. This helper
    /// compares the request's current authorization fingerprint instead.
    pub fn authorize(&self, current_fingerprint: &str) -> Result<(), EntryCursorError> {
        if self.authorization_fingerprint == current_fingerprint {
            Ok(())
        } else {
            Err(EntryCursorError::AuthorizationChanged)
        }
    }
}

#[derive(Debug)]
pub enum EntryQueryError {
    Invalid(String),
    PropertyRequiresFormScope,
    Serialize(serde_json::Error),
}

impl fmt::Display for EntryQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::PropertyRequiresFormScope => {
                formatter.write_str("property fields require a Form-scoped Entry query")
            }
            Self::Serialize(error) => write!(formatter, "serialize Entry query: {error}"),
        }
    }
}

impl std::error::Error for EntryQueryError {}

#[derive(Debug)]
pub enum EntryCursorError {
    Invalid(String),
    Tampered,
    AuthorizationChanged,
    Serialize(serde_json::Error),
    Deserialize(serde_json::Error),
}

impl fmt::Display for EntryCursorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::Tampered => formatter.write_str("Entry cursor signature is invalid"),
            Self::AuthorizationChanged => {
                formatter.write_str("Entry cursor authorization has changed")
            }
            Self::Serialize(error) => write!(formatter, "serialize Entry cursor: {error}"),
            Self::Deserialize(error) => write!(formatter, "decode Entry cursor: {error}"),
        }
    }
}

impl std::error::Error for EntryCursorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::space_key::SpaceUri;
    use uuid::Uuid;

    fn publication() -> PublicationRef {
        PublicationRef::new(
            7,
            SpaceUri::parse("ugoite://018f6c7e-5f6a-7b8c-9d0e-1f2a3b4c5d6e/publications/7")
                .expect("URI"),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("publication")
    }

    fn query() -> EntryQuery {
        EntryQuery {
            scope: EntryQueryScope::Form {
                form_id: FormId::from(Uuid::from_u128(2)),
            },
            text: Some("alice".into()),
            filters: vec![EntryFilter {
                field: EntryFieldRef::Property {
                    field_id: FieldId::new(100).expect("field id"),
                },
                operator: SearchOperator::Equals,
                value: Value::String("active".into()),
            }],
            sort: vec![EntrySort {
                field: EntryFieldRef::UpdatedAt,
                direction: EntrySortDirection::Desc,
            }],
        }
    }

    #[test]
    fn property_fields_require_form_scope() {
        let mut query = query();
        query.scope = EntryQueryScope::All;
        assert_eq!(
            query
                .validate()
                .expect_err("All Forms property must fail")
                .to_string(),
            "property fields require a Form-scoped Entry query"
        );
    }

    #[test]
    fn projection_does_not_change_query_fingerprint() {
        let query = query();
        let first = query.fingerprint().expect("fingerprint");
        let request = EntryPageRequest {
            query,
            projection: EntryProjection::Preview,
            limit: 50,
            after: None,
        };
        request.validate().expect("request");
        assert_eq!(first, request.query.fingerprint().expect("fingerprint"));
    }

    #[test]
    fn all_forms_projection_rejects_property_fields() {
        let request = EntryPageRequest {
            query: EntryQuery::default(),
            projection: EntryProjection::Fields {
                fields: vec![EntryFieldRef::Property {
                    field_id: FieldId::new(100).expect("field id"),
                }],
            },
            limit: 50,
            after: None,
        };
        assert!(matches!(
            request.validate(),
            Err(EntryQueryError::PropertyRequiresFormScope)
        ));
    }

    #[test]
    fn signed_cursor_rejects_tampering_and_detects_auth_changes() {
        let cursor = EntryCursor::new(
            SpaceId::from(Uuid::from_u128(1)),
            publication(),
            query().fingerprint().expect("fingerprint"),
            "auth-v1".into(),
            vec![Value::String("2026-01-01T00:00:00Z".into())],
            Some(FormId::from(Uuid::from_u128(2))),
            EntryId::from(Uuid::from_u128(3)),
        )
        .expect("cursor");
        let token = cursor.encode(b"test-signing-key").expect("encode");
        let decoded = EntryCursor::decode(&token, b"test-signing-key").expect("decode");
        assert_eq!(decoded, cursor);
        assert!(matches!(
            EntryCursor::decode(&(token + "x"), b"test-signing-key"),
            Err(EntryCursorError::Tampered)
        ));
        assert!(matches!(
            decoded.authorize("auth-v2"),
            Err(EntryCursorError::AuthorizationChanged)
        ));
    }
}
