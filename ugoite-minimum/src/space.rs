use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SpaceMeta {
    pub id: String,
    pub name: String,
    pub created_at: f64,
    pub storage: StorageConfig,
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
