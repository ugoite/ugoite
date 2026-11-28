use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct MetadataRegistry {
    columns: Vec<String>,
    forms: Vec<String>,
}

static METADATA_REGISTRY: OnceLock<MetadataRegistry> = OnceLock::new();

fn registry() -> &'static MetadataRegistry {
    METADATA_REGISTRY.get_or_init(|| {
        serde_json::from_str(include_str!("metadata.json"))
            .expect("the domain metadata registry must be valid JSON")
    })
}

pub fn is_reserved_metadata_column(name: &str) -> bool {
    let name = name.trim();
    registry()
        .columns
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

pub fn is_reserved_metadata_form(name: &str) -> bool {
    let name = name.trim();
    registry()
        .forms
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}
