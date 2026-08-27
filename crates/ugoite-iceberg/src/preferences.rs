use anyhow::{anyhow, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use ugoite_storage::{
    CasOutcome, CreateOutcome, OpendalPublicationStore, OpendalStorage, PublicationError,
    PublicationStore, SpaceKey, StorageBackend,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalePreference {
    En,
    Ja,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiThemePreference {
    Materialize,
    Classic,
    Pop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorModePreference {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimaryColorPreference {
    Violet,
    Blue,
    Emerald,
    Amber,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentWidthPreference {
    Standard,
    Wide,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserPreferences {
    pub selected_space_id: Option<String>,
    pub locale: Option<LocalePreference>,
    pub ui_theme: Option<UiThemePreference>,
    pub color_mode: Option<ColorModePreference>,
    pub primary_color: Option<PrimaryColorPreference>,
    pub content_width: Option<ContentWidthPreference>,
}

const USER_PREFERENCE_FIELDS: &[&str] = &[
    "selected_space_id",
    "locale",
    "ui_theme",
    "color_mode",
    "primary_color",
    "content_width",
];

fn hashed_user_segment(user_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    hex::encode(hasher.finalize())
}

fn preferences_path_for_hash(user_hash: &str) -> String {
    format!("users/{user_hash}/preferences.json")
}

fn preferences_path(user_id: &str) -> String {
    let user_hash = hashed_user_segment(user_id);
    preferences_path_for_hash(&user_hash)
}

fn validate_patch(patch: &Value) -> Result<&serde_json::Map<String, Value>> {
    let Some(patch_obj) = patch.as_object() else {
        return Err(anyhow!("preferences patch must be a JSON object"));
    };

    for key in patch_obj.keys() {
        if !USER_PREFERENCE_FIELDS.contains(&key.as_str()) {
            return Err(anyhow!("Unknown preference field: {key}"));
        }
    }

    Ok(patch_obj)
}

async fn get_user_preferences_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    user_id: &str,
) -> Result<UserPreferences> {
    let path = preferences_path(user_id);
    if !storage.exists(&path).await? {
        return Ok(UserPreferences::default());
    }
    storage.read_json(&path).await
}

pub async fn get_user_preferences(op: &Operator, user_id: &str) -> Result<UserPreferences> {
    let storage = OpendalStorage::from_operator(op);
    get_user_preferences_with_storage(&storage, user_id).await
}

async fn patch_user_preferences_with_storage(
    op: &Operator,
    user_id: &str,
    patch: &Value,
) -> Result<UserPreferences> {
    let patch_obj = validate_patch(patch)?;
    let path = preferences_path(user_id);
    let key = SpaceKey::parse(&path)?;
    let publication_store = OpendalPublicationStore::new(op.clone());

    for _attempt in 0..5 {
        let current = publication_store
            .load(&key)
            .await
            .map_err(|error| anyhow!(error))?;
        let mut merged = match current.as_ref() {
            Some(current) => serde_json::from_slice(&current.bytes)?,
            None => serde_json::to_value(UserPreferences::default())?,
        };
        let Some(merged_obj) = merged.as_object_mut() else {
            return Err(anyhow!("preferences payload must serialize to an object"));
        };

        for (key, value) in patch_obj {
            merged_obj.insert(key.clone(), value.clone());
        }

        let preferences: UserPreferences = serde_json::from_value(merged)?;
        let bytes = serde_json::to_vec_pretty(&preferences)?;
        let outcome = match current.as_ref() {
            Some(current) => publication_store
                .compare_and_swap(&key, &current.revision, bytes.clone())
                .await
                .map(|outcome| match outcome {
                    CasOutcome::Replaced => CreateOutcome::Created,
                    CasOutcome::RevisionMismatch => CreateOutcome::AlreadyExists,
                }),
            None => publication_store.create(&key, bytes.clone()).await,
        };

        match outcome {
            Ok(CreateOutcome::Created) => return Ok(preferences),
            Ok(CreateOutcome::AlreadyExists) => {
                if current.is_some() {
                    continue;
                }
            }
            Err(PublicationError::OutcomeUnknown(error)) => {
                let committed = publication_store
                    .load(&key)
                    .await
                    .map_err(|reread| anyhow!(reread))?
                    .is_some_and(|observed| observed.bytes == bytes);
                if committed {
                    return Ok(preferences);
                }
                return Err(error.context("preference mutation outcome is unknown"));
            }
            Err(error) => return Err(anyhow!(error)),
        }
    }

    Err(anyhow!("preferences update conflicted with another writer"))
}

pub async fn patch_user_preferences(
    op: &Operator,
    user_id: &str,
    patch: &Value,
) -> Result<UserPreferences> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    patch_user_preferences_with_storage(op, user_id, patch).await
}
