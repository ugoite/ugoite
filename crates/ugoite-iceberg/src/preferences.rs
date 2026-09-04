use anyhow::{anyhow, bail, Result};
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserPreferences {
    pub selected_space_id: Option<String>,
    pub locale: Option<LocalePreference>,
}

const USER_PREFERENCE_FIELDS: &[&str] = &["selected_space_id", "locale"];

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

async fn patch_user_preferences_with_publication(
    operator: &Operator,
    user_id: &str,
    patch: &Value,
) -> Result<UserPreferences> {
    let patch_obj = validate_patch(patch)?;
    let user_hash = hashed_user_segment(user_id);
    let key = SpaceKey::parse(&preferences_path_for_hash(&user_hash))?;
    let publication = OpendalPublicationStore::new(operator.clone());

    // Preferences are an operator-level mutable object rather than a Catalog
    // Head. Keep their merge operation on the same exact-read/CAS contract so
    // concurrent patches cannot silently discard one another.
    const MAX_CAS_ATTEMPTS: usize = 8;
    for _attempt in 0..MAX_CAS_ATTEMPTS {
        let current = publication
            .load(&key)
            .await
            .map_err(|error| anyhow!("load user preferences: {error}"))?;
        let current_preferences = match current.as_ref() {
            Some(object) => serde_json::from_slice::<UserPreferences>(&object.bytes)?,
            None => UserPreferences::default(),
        };
        let mut merged = serde_json::to_value(current_preferences)?;
        let Some(merged_obj) = merged.as_object_mut() else {
            return Err(anyhow!("preferences payload must serialize to an object"));
        };
        for (key, value) in patch_obj {
            merged_obj.insert(key.clone(), value.clone());
        }
        let preferences: UserPreferences = serde_json::from_value(merged)?;
        let bytes = serde_json::to_vec_pretty(&preferences)?;

        let write_result = match current.as_ref() {
            None => publication
                .create(&key, bytes)
                .await
                .map(|outcome| match outcome {
                    CreateOutcome::Created => Ok(()),
                    CreateOutcome::AlreadyExists => Err(()),
                }),
            Some(object) => publication
                .compare_and_swap(&key, &object.revision, bytes)
                .await
                .map(|outcome| match outcome {
                    CasOutcome::Replaced => Ok(()),
                    CasOutcome::RevisionMismatch => Err(()),
                }),
        };

        match write_result {
            Ok(Ok(())) => return Ok(preferences),
            Ok(Err(())) => continue,
            Err(PublicationError::OutcomeUnknown(write_error)) => {
                // A transport error may occur after the backend committed the
                // write. Reconcile by exact read before retrying or reporting
                // failure; this preserves the successful result when known.
                match publication.load(&key).await {
                    Ok(Some(object)) => {
                        let observed: UserPreferences = serde_json::from_slice(&object.bytes)?;
                        if observed == preferences {
                            return Ok(preferences);
                        }
                    }
                    Ok(None) => {}
                    Err(read_error) => {
                        return Err(anyhow!(
                            "preference write outcome is unknown and exact reconciliation failed: {write_error}; {read_error}"
                        ));
                    }
                }
            }
            Err(PublicationError::InvalidKey(error)) => return Err(error.into()),
            Err(PublicationError::Backend(error)) => {
                return Err(anyhow!("write user preferences: {error}"));
            }
        }
    }

    bail!("user preferences changed concurrently; retry the operation")
}

pub async fn patch_user_preferences(
    op: &Operator,
    user_id: &str,
    patch: &Value,
) -> Result<UserPreferences> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    ugoite_storage::verify_publication_mutation_contract(op)
        .await
        .map_err(crate::iceberg_store::storage_mutation_unavailable)?;
    patch_user_preferences_with_publication(op, user_id, patch).await
}
