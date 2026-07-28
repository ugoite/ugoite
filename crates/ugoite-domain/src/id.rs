use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(SpaceId);
uuid_id!(FormId);
uuid_id!(EntryId);
uuid_id!(RevisionId);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldId(i32);

impl FieldId {
    pub fn new(value: i32) -> Result<Self, FieldIdError> {
        if value < 100 {
            return Err(FieldIdError);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FieldIdError;

impl fmt::Display for FieldIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("field ID must be at least 100; lower IDs are reserved")
    }
}

impl std::error::Error for FieldIdError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IdentifierKind {
    Space,
    Checkpoint,
    Entry,
    Form,
    Asset,
    Sql,
    SqlSession,
    Revision,
}

impl IdentifierKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Space => "space_id",
            Self::Checkpoint => "checkpoint_name",
            Self::Entry => "entry_id",
            Self::Form => "form_name",
            Self::Asset => "asset_id",
            Self::Sql => "sql_id",
            Self::SqlSession => "sql_session_id",
            Self::Revision => "revision_id",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IdentifierError {
    kind: IdentifierKind,
    value: String,
    reason: &'static str,
}

impl IdentifierError {
    pub fn kind(&self) -> IdentifierKind {
        self.kind
    }

    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid {}: {}", self.kind.as_str(), self.reason)
    }
}

impl std::error::Error for IdentifierError {}

pub fn validate_identifier(kind: IdentifierKind, value: &str) -> Result<(), IdentifierError> {
    let decoded = percent_decode_lossy(value);
    let reason = invalid_reason(value, &decoded);
    match reason {
        Some(reason) => Err(IdentifierError {
            kind,
            value: value.to_string(),
            reason,
        }),
        None => Ok(()),
    }
}

pub fn validate_space_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Space, value)
}

pub fn validate_checkpoint_name(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Checkpoint, value)
}

pub fn validate_entry_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Entry, value)
}

pub fn validate_form_name(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Form, value)
}

pub fn validate_asset_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Asset, value)
}

pub fn validate_sql_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Sql, value)
}

pub fn validate_sql_session_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::SqlSession, value)
}

pub fn validate_revision_id(value: &str) -> Result<(), IdentifierError> {
    validate_identifier(IdentifierKind::Revision, value)
}

fn invalid_reason(value: &str, decoded: &str) -> Option<&'static str> {
    if value.is_empty() {
        return Some("must not be empty");
    }
    if value.len() > 128 {
        return Some("must be at most 128 bytes");
    }
    if decoded == "." || decoded == ".." {
        return Some("dot segments are not allowed");
    }
    if decoded.contains('/') || decoded.contains('\\') {
        return Some("path separators are not allowed");
    }
    if decoded.chars().any(char::is_control) {
        return Some("control characters are not allowed");
    }
    if !decoded
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Some("only ASCII letters, digits, '-' and '_' are allowed");
    }
    None
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_form_name, validate_space_id};

    #[test]
    fn rejects_unsafe_storage_path_segments() {
        for value in [
            "",
            ".",
            "..",
            "../x",
            "x/../y",
            "x\\y",
            "%2e%2e",
            "%2f",
            "%5c",
            "bad\u{0000}id",
            "bad.id",
        ] {
            assert!(validate_space_id(value).is_err(), "{value:?}");
            assert!(validate_form_name(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn accepts_documented_safe_identifiers() {
        for value in ["default", "operations", "Entry", "SQL_2026", "id-123_ABC"] {
            validate_space_id(value).unwrap();
            validate_form_name(value).unwrap();
        }
    }

    #[test]
    fn rejects_overly_long_identifier() {
        let value = "a".repeat(129);
        assert!(validate_space_id(&value).is_err());
    }
}
