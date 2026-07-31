use crate::link::Link;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub form: String,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default)]
    pub properties: Value,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub assets: Vec<Value>,
    #[serde(default)]
    pub checksum: String,
}
