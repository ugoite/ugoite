use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;
use uuid::Uuid;

/// A canonical coordinate relative to one bound Space.
///
/// `SpaceKey` deliberately models a logical object name rather than a URI or
/// a filesystem path.  It rejects every representation that could make the
/// same object address ambiguous across local filesystems and object stores.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SpaceKey(String);

impl<'de> Deserialize<'de> for SpaceKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpaceKeyError {
    reason: &'static str,
}

impl SpaceKeyError {
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for SpaceKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid Space key: {}", self.reason)
    }
}

impl std::error::Error for SpaceKeyError {}

impl SpaceKey {
    pub fn parse(value: &str) -> Result<Self, SpaceKeyError> {
        if value.is_empty() {
            return Err(Self::error("key must not be empty"));
        }
        if value.starts_with('/')
            || value.ends_with('/')
            || value.contains("//")
            || value.contains('\\')
            || value.contains('?')
            || value.contains('#')
            || value.contains('%')
            || value.contains("://")
        {
            return Err(Self::error("key is not a canonical relative coordinate"));
        }

        for segment in value.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(Self::error("key contains an empty or dot segment"));
            }
            if segment.contains(':') {
                return Err(Self::error("key contains a URI-like segment"));
            }
        }

        Ok(Self(value.to_owned()))
    }

    pub fn meta() -> Self {
        Self::from_static("meta.json")
    }

    pub fn settings() -> Self {
        Self::from_static("settings.json")
    }

    pub fn catalog_head() -> Self {
        Self::from_static("_ugoite/catalog/head.json")
    }

    pub fn asset(asset_id: &str) -> Result<Self, SpaceKeyError> {
        Self::parse(&format!("assets/{asset_id}"))
    }

    pub fn form_metadata(form_id: &str, version: u64) -> Result<Self, SpaceKeyError> {
        Self::parse(&format!("forms/{form_id}/metadata/{version:05}.json"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    fn from_static(value: &'static str) -> Self {
        // Every static constructor is maintained in this module and therefore
        // goes through the same invariant as externally parsed keys.
        Self(value.to_owned())
    }

    const fn error(reason: &'static str) -> SpaceKeyError {
        SpaceKeyError { reason }
    }
}

impl TryFrom<&str> for SpaceKey {
    type Error = SpaceKeyError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for SpaceKey {
    type Error = SpaceKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl fmt::Display for SpaceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A portable URI for a Space-relative coordinate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SpaceUri {
    space_uid: Uuid,
    key: SpaceKey,
}

impl<'de> Deserialize<'de> for SpaceUri {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            space_uid: Uuid,
            key: SpaceKey,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.space_uid, wire.key).map_err(serde::de::Error::custom)
    }
}

impl SpaceUri {
    pub fn new(space_uid: Uuid, key: SpaceKey) -> Result<Self, SpaceKeyError> {
        if space_uid.get_version() != Some(uuid::Version::SortRand) {
            return Err(SpaceKey::error("Space URI identity must be a UUIDv7"));
        }
        Ok(Self { space_uid, key })
    }

    pub fn parse(value: &str) -> Result<Self, SpaceKeyError> {
        let raw_rest = value
            .strip_prefix("ugoite://")
            .ok_or_else(|| SpaceKey::error("logical URI must use the ugoite scheme"))?;
        if raw_rest.contains('%') {
            return Err(SpaceKey::error(
                "logical URI must not use percent-encoded coordinates",
            ));
        }
        if let Some((_, raw_path)) = raw_rest.split_once('/') {
            if raw_path.is_empty()
                || raw_path.starts_with('/')
                || raw_path.ends_with('/')
                || raw_path.contains("//")
                || raw_path.contains("/./")
                || raw_path.contains("/../")
                || raw_path == "."
                || raw_path == ".."
                || raw_path.starts_with("./")
                || raw_path.starts_with("../")
            {
                return Err(SpaceKey::error("logical URI contains an ambiguous path"));
            }
        }
        let uri = Url::parse(value).map_err(|_| SpaceKey::error("invalid logical URI"))?;
        if uri.scheme() != "ugoite"
            || uri.query().is_some()
            || uri.fragment().is_some()
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.port().is_some()
        {
            return Err(SpaceKey::error(
                "logical URI contains unsupported components",
            ));
        }
        let host = uri
            .host_str()
            .ok_or_else(|| SpaceKey::error("logical URI has no Space identity"))?;
        let space_uid = Uuid::parse_str(host)
            .map_err(|_| SpaceKey::error("logical URI Space identity is not a UUID"))?;
        let key = SpaceKey::parse(uri.path().strip_prefix('/').unwrap_or_default())?;
        Self::new(space_uid, key)
    }

    pub fn space_uid(&self) -> Uuid {
        self.space_uid
    }

    pub fn key(&self) -> &SpaceKey {
        &self.key
    }
}

impl fmt::Display for SpaceUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ugoite://{}/{}", self.space_uid, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid() -> Uuid {
        Uuid::parse_str("018f6c7e-5f6a-7b8c-9d0e-1f2a3b4c5d6e").unwrap()
    }

    #[test]
    fn rejects_ambiguous_or_escaping_keys() {
        for key in [
            "",
            "/meta.json",
            "meta.json/",
            "forms//metadata.json",
            "forms/./metadata.json",
            "forms/../meta.json",
            "forms\\metadata.json",
            "forms/%2e%2e/meta.json",
            "s3://bucket/meta.json",
            "meta.json?x=1",
        ] {
            assert!(SpaceKey::parse(key).is_err(), "accepted {key:?}");
        }
    }

    #[test]
    fn logical_uri_round_trips_and_binds_identity() {
        let uri = SpaceUri::new(uid(), SpaceKey::meta()).unwrap();
        let encoded = uri.to_string();
        assert_eq!(SpaceUri::parse(&encoded).unwrap().to_string(), encoded);
        assert!(SpaceUri::parse(&format!("{encoded}?backend=s3")).is_err());
        assert!(SpaceUri::parse(&format!("ugoite://{}/../meta.json", uid())).is_err());
    }

    #[test]
    fn serde_rejects_unvalidated_space_coordinates() {
        let invalid_key = serde_json::json!({
            "space_uid": uid(),
            "key": "../meta.json"
        });
        assert!(serde_json::from_value::<SpaceUri>(invalid_key).is_err());

        let invalid_identity = serde_json::json!({
            "space_uid": Uuid::from_u128(1),
            "key": "meta.json"
        });
        assert!(serde_json::from_value::<SpaceUri>(invalid_identity).is_err());
    }

    #[test]
    fn generated_keys_are_canonical() {
        assert_eq!(
            SpaceKey::catalog_head().as_str(),
            "_ugoite/catalog/head.json"
        );
        assert_eq!(
            SpaceKey::asset("asset-1").unwrap().as_str(),
            "assets/asset-1"
        );
        assert_eq!(
            SpaceKey::form_metadata("form-1", 7).unwrap().as_str(),
            "forms/form-1/metadata/00007.json"
        );
    }
}
