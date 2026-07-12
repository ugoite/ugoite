use serde::{Deserialize, Serialize};
use ugoite_domain::id::{FormId, SpaceId};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub format_version: u32,
    pub space_id: SpaceId,
    pub source_backup: String,
    pub forms: Vec<MigrationFormReport>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationFormReport {
    pub form_id: FormId,
    pub form_name: String,
    pub source_entry_count: u64,
    pub source_revision_count: u64,
    pub migrated_revision_count: u64,
    pub tombstone_count: u64,
    pub field_mapping_complete: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationReport {
    pub manifest: MigrationManifest,
    pub dry_run: bool,
    pub logical_data_matches: bool,
    pub errors: Vec<String>,
}

impl MigrationReport {
    pub fn verify(manifest: MigrationManifest, dry_run: bool) -> Self {
        let mut errors = Vec::new();
        for form in &manifest.forms {
            if form.source_revision_count != form.migrated_revision_count {
                errors.push(format!(
                    "{} revision count differs: {} != {}",
                    form.form_name, form.source_revision_count, form.migrated_revision_count
                ));
            }
            if !form.field_mapping_complete {
                errors.push(format!(
                    "{} has an incomplete field mapping",
                    form.form_name
                ));
            }
        }
        Self {
            manifest,
            dry_run,
            logical_data_matches: errors.is_empty(),
            errors,
        }
    }
}
