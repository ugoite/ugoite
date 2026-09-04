mod common;

use common::setup_operator;
use serde_json::json;
use sha2::{Digest, Sha256};
use ugoite_iceberg::preferences;

#[tokio::test]
/// REQ-STO-011
async fn test_preferences_req_sto_011_default_values() -> anyhow::Result<()> {
    let op = setup_operator()?;

    let preferences = preferences::get_user_preferences(&op, "user@example.com").await?;

    assert_eq!(preferences.selected_space_id, None);
    assert_eq!(preferences.locale, None);

    Ok(())
}

#[tokio::test]
/// REQ-STO-011
async fn test_preferences_req_sto_011_patch_roundtrip_uses_hashed_user_path() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    let user_id = "unsafe/user@example.com";

    let initial = preferences::patch_user_preferences(
        &op,
        user_id,
        &json!({
            "selected_space_id": "space-1",
            "locale": "ja"
        }),
    )
    .await?;

    assert_eq!(initial.selected_space_id.as_deref(), Some("space-1"));
    assert_eq!(initial.locale, Some(preferences::LocalePreference::Ja));

    let updated = preferences::patch_user_preferences(
        &op,
        user_id,
        &json!({
            "selected_space_id": "space-2"
        }),
    )
    .await?;

    assert_eq!(updated.selected_space_id.as_deref(), Some("space-2"));
    assert_eq!(updated.locale, Some(preferences::LocalePreference::Ja));

    let user_hash = hex::encode(Sha256::digest(user_id.as_bytes()));
    let hashed_path = format!("users/{user_hash}/preferences.json");
    let raw_path = format!("users/{user_id}/preferences.json");
    assert!(op.exists(&hashed_path).await?);
    assert!(!op.exists(&raw_path).await?);

    let stored = preferences::get_user_preferences(&op, user_id).await?;
    assert_eq!(stored.selected_space_id.as_deref(), Some("space-2"));
    assert_eq!(stored.locale, Some(preferences::LocalePreference::Ja));

    Ok(())
}

#[tokio::test]
async fn retired_appearance_preferences_are_rejected() -> anyhow::Result<()> {
    let op = setup_operator()?;

    let error = preferences::patch_user_preferences(
        &op,
        "appearance-user",
        &json!({"ui_theme": "classic"}),
    )
    .await
    .expect_err("retired product theme preferences must not be accepted");

    assert!(error
        .to_string()
        .contains("Unknown preference field: ui_theme"));
    Ok(())
}

#[tokio::test]
async fn concurrent_preference_patches_preserve_both_updates() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let locale_patch = json!({"locale": "ja"});
    let selected_space_patch = json!({"selected_space_id": "space-2"});

    let (locale, selected_space) = tokio::join!(
        preferences::patch_user_preferences(&op, "concurrent-user", &locale_patch),
        preferences::patch_user_preferences(&op, "concurrent-user", &selected_space_patch,),
    );
    locale?;
    selected_space?;

    let stored = preferences::get_user_preferences(&op, "concurrent-user").await?;
    assert_eq!(stored.locale, Some(preferences::LocalePreference::Ja));
    assert_eq!(stored.selected_space_id.as_deref(), Some("space-2"));

    Ok(())
}
