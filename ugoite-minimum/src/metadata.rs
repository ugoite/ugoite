use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, OnceLock};

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
    "integrity",
    "space_id",
    "word_count",
];

const DEFAULT_METADATA_FORMS: &[&str] = &["SQL", "Assets"];

static METADATA_COLUMNS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static METADATA_FORMS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn build_metadata_columns_store() -> Mutex<HashSet<String>> {
    let mut set = HashSet::new();
    for name in DEFAULT_METADATA_COLUMNS {
        set.insert(name.to_string());
    }
    Mutex::new(set)
}

fn build_metadata_forms_store() -> Mutex<HashSet<String>> {
    let mut set = HashSet::new();
    for name in DEFAULT_METADATA_FORMS {
        set.insert(name.trim().to_lowercase());
    }
    Mutex::new(set)
}

fn metadata_columns_store() -> &'static Mutex<HashSet<String>> {
    METADATA_COLUMNS.get_or_init(build_metadata_columns_store)
}
fn metadata_forms_store() -> &'static Mutex<HashSet<String>> {
    METADATA_FORMS.get_or_init(build_metadata_forms_store)
}
fn metadata_columns_guard() -> MutexGuard<'static, HashSet<String>> {
    metadata_columns_store()
        .lock()
        .expect("metadata columns registry lock poisoned")
}
fn metadata_forms_guard() -> MutexGuard<'static, HashSet<String>> {
    metadata_forms_store()
        .lock()
        .expect("metadata forms registry lock poisoned")
}
pub fn metadata_columns() -> HashSet<String> {
    metadata_columns_guard().clone()
}
pub fn is_reserved_metadata_column(name: &str) -> bool {
    let set = metadata_columns_guard();
    for reserved in set.iter() {
        if reserved.eq_ignore_ascii_case(name) {
            return true;
        }
    }
    false
}
pub fn metadata_forms() -> HashSet<String> {
    metadata_forms_guard().clone()
}
pub fn is_reserved_metadata_form(name: &str) -> bool {
    let set = metadata_forms_guard();
    for reserved in set.iter() {
        if reserved.eq_ignore_ascii_case(name) {
            return true;
        }
    }
    false
}

pub fn register_metadata_columns<I>(columns: I)
where
    I: IntoIterator<Item = String>,
{
    let mut set = metadata_columns_guard();
    for column in columns {
        set.insert(column);
    }
}

pub fn register_metadata_forms<I>(forms: I)
where
    I: IntoIterator<Item = String>,
{
    let mut store = metadata_forms_guard();
    for name in forms {
        store.insert(name.trim().to_lowercase());
    }
}
