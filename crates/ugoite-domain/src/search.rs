use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct KeywordSearchResult {
    pub id: String,
    pub title: String,
    pub form: String,
    pub created_at: f64,
    pub updated_at: f64,
}
