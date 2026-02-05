use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

const DEFAULT_METADATA_COLUMNS: &[&str] = &[
    "id",
    "entry_id",
    "title",
    "form",
    "tags",
    "links",
    "assets",
    "created_at",
    "updated_at",
    "revision_id",
    "parent_revision_id",
    "deleted",
    "deleted_at",
    "author",
    "canvas_position",
    "integrity",
    "space_id",
    "word_count",
];

const DEFAULT_METADATA_FORMS: &[&str] = &["SQL"];

static METADATA_COLUMNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static METADATA_FORMS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn metadata_columns_store() -> &'static Mutex<HashSet<String>> {
    METADATA_COLUMNS.get_or_init(|| {
        let mut set = HashSet::new();
        for name in DEFAULT_METADATA_COLUMNS {
            set.insert(name.to_string());
        }
        Mutex::new(set)
    })
}

fn metadata_forms_store() -> &'static Mutex<HashSet<String>> {
    METADATA_FORMS.get_or_init(|| {
        let mut set = HashSet::new();
        for name in DEFAULT_METADATA_FORMS {
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

pub fn metadata_forms() -> HashSet<String> {
    metadata_forms_store()
        .lock()
        .map(|set| set.clone())
        .unwrap_or_default()
}

pub fn is_reserved_metadata_form(name: &str) -> bool {
    metadata_forms_store()
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

pub fn register_metadata_forms<I>(forms: I)
where
    I: IntoIterator<Item = String>,
{
    if let Ok(mut store) = metadata_forms_store().lock() {
        for name in forms {
            store.insert(name.trim().to_lowercase());
        }
    }
}
