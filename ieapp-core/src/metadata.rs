use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

const DEFAULT_METADATA_COLUMNS: &[&str] = &[
    "id",
    "note_id",
    "title",
    "class",
    "tags",
    "links",
    "attachments",
    "created_at",
    "updated_at",
    "revision_id",
    "parent_revision_id",
    "deleted",
    "deleted_at",
    "author",
    "canvas_position",
    "integrity",
    "workspace_id",
    "word_count",
];

const DEFAULT_METADATA_CLASSES: &[&str] = &["SQL"];

static METADATA_COLUMNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static METADATA_CLASSES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn metadata_columns_store() -> &'static Mutex<HashSet<String>> {
    METADATA_COLUMNS.get_or_init(|| {
        let mut set = HashSet::new();
        for name in DEFAULT_METADATA_COLUMNS {
            set.insert(name.to_string());
        }
        Mutex::new(set)
    })
}

fn metadata_classes_store() -> &'static Mutex<HashSet<String>> {
    METADATA_CLASSES.get_or_init(|| {
        let mut set = HashSet::new();
        for name in DEFAULT_METADATA_CLASSES {
            set.insert(name.trim().to_lowercase());
        }
        Mutex::new(set)
    })
}

pub fn metadata_columns() -> HashSet<String> {
    metadata_columns_store()
        .lock()
        .map(|set| set.clone())
        .unwrap_or_default()
}

pub fn is_reserved_metadata_column(name: &str) -> bool {
    metadata_columns_store()
        .lock()
        .map(|set| {
            set.iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(name))
        })
        .unwrap_or(false)
}

pub fn metadata_classes() -> HashSet<String> {
    metadata_classes_store()
        .lock()
        .map(|set| set.clone())
        .unwrap_or_default()
}

pub fn is_reserved_metadata_class(name: &str) -> bool {
    metadata_classes_store()
        .lock()
        .map(|set| {
            set.iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(name))
        })
        .unwrap_or(false)
}

pub fn register_metadata_columns<I>(columns: I)
where
    I: IntoIterator<Item = String>,
{
    if let Ok(mut set) = metadata_columns_store().lock() {
        for column in columns {
            set.insert(column);
        }
    }
}

pub fn register_metadata_classes<I>(classes: I)
where
    I: IntoIterator<Item = String>,
{
    if let Ok(mut store) = metadata_classes_store().lock() {
        for name in classes {
            store.insert(name.trim().to_lowercase());
        }
    }
}
