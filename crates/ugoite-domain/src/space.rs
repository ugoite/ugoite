use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SpaceMeta {
    pub space_uid: Uuid,
    pub slug: String,
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub space_version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: String,
    pub root: String,
}

pub fn storage_type_and_root(root_uri: &str) -> (String, String, String) {
    if let Ok(url) = Url::parse(root_uri) {
        let scheme = url.scheme().to_string();
        let root = if scheme == "fs" || scheme == "file" {
            url.path().to_string()
        } else {
            url.path().trim_start_matches('/').to_string()
        };
        let storage_type = if scheme == "fs" || scheme == "file" {
            "local".to_string()
        } else {
            scheme.clone()
        };
        return (storage_type, root, scheme);
    }

    (
        "local".to_string(),
        root_uri.to_string(),
        "file".to_string(),
    )
}

/// Single Space display-name normalization rule.
///
/// Creation callers trim one provided display name and reject an
/// empty/whitespace-only value before any write; an absent name defaults to
/// the requested slug at the call site (never here). The positional slug
/// stays the stable Space key while the normalized name only seeds
/// `meta.json:name` on first creation; retries never rename.
pub fn normalize_space_display_name(value: &str) -> Result<String, SpaceDisplayNameError> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(SpaceDisplayNameError);
    }
    Ok(normalized)
}

/// Rejection of an empty/whitespace-only Space display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceDisplayNameError;

impl std::fmt::Display for SpaceDisplayNameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Space display name must not be empty")
    }
}

impl std::error::Error for SpaceDisplayNameError {}

/// Stable durable Space compatibility identity.
///
/// Product versions and Space versions evolve independently. A Space version
/// changes only when the meaning of durable Space data changes incompatibly;
/// physical storage representation and local subsystem formats do not define
/// this contract.
pub const CURRENT_SPACE_VERSION: &str = "0.1";

/// Space compatibility versions supported by this Product.
pub const SUPPORTED_SPACE_VERSIONS: &[&str] = &[CURRENT_SPACE_VERSION];

/// Parsed Space compatibility version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpaceVersion {
    pub major: u64,
    pub generation: u64,
}

impl std::fmt::Display for SpaceVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}", self.major, self.generation)
    }
}

/// Failure to classify a Space compatibility version before structural
/// validation. Callers must fail closed and must not infer a version from
/// another metadata field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceVersionError {
    Missing,
    Malformed { detected: String },
    Unsupported { detected: String },
}

impl SpaceVersionError {
    pub fn detected(&self) -> Option<&str> {
        match self {
            Self::Missing => None,
            Self::Malformed { detected } | Self::Unsupported { detected } => Some(detected),
        }
    }
}

impl std::fmt::Display for SpaceVersionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("Space metadata is missing space_version"),
            Self::Malformed { detected } => {
                write!(
                    formatter,
                    "Space metadata has malformed space_version: {detected}"
                )
            }
            Self::Unsupported { detected } => {
                write!(formatter, "Space version is unsupported: {detected}")
            }
        }
    }
}

impl std::error::Error for SpaceVersionError {}

/// Parse the canonical `<major>.<generation>` Space version shape.
pub fn parse_space_version(value: &str) -> Option<SpaceVersion> {
    let (major, generation) = value.split_once('.')?;
    if major.is_empty()
        || generation.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !generation.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let version = SpaceVersion {
        major: major.parse().ok()?,
        generation: generation.parse().ok()?,
    };
    (version.to_string() == value).then_some(version)
}

/// Classify the durable Space compatibility identity.
///
/// This is the only supported-version authority for Space bootstrap metadata.
/// It deliberately ignores `schema_version`: subsystem-local format fields
/// cannot be promoted to the portable Space compatibility contract.
pub fn classify_space_version(
    metadata: &serde_json::Value,
) -> Result<SpaceVersion, SpaceVersionError> {
    let detected = match metadata.get("space_version") {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(value) => {
            return Err(SpaceVersionError::Malformed {
                detected: serde_json::to_string(value).unwrap_or_else(|_| "malformed".to_string()),
            });
        }
        None => return Err(SpaceVersionError::Missing),
    };
    let parsed = parse_space_version(&detected).ok_or_else(|| SpaceVersionError::Malformed {
        detected: detected.clone(),
    })?;
    // Do not normalize non-canonical values into a supported identity.
    if parsed.to_string() != detected {
        return Err(SpaceVersionError::Malformed { detected });
    }
    if !SUPPORTED_SPACE_VERSIONS.contains(&detected.as_str()) {
        return Err(SpaceVersionError::Unsupported { detected });
    }
    Ok(parsed)
}
