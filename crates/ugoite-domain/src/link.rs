use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
}
