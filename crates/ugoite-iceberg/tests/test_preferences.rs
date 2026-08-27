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
    assert_eq!(preferences.ui_theme, None);
    assert_eq!(preferences.color_mode, None);
    assert_eq!(preferences.primary_color, None);
    assert_eq!(preferences.content_width, None);

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
            "locale": "ja",
            "ui_theme": "classic"
        }),
    )
    .await?;

    assert_eq!(initial.selected_space_id.as_deref(), Some("space-1"));
    assert_eq!(initial.locale, Some(preferences::LocalePreference::Ja));
    assert_eq!(
        initial.ui_theme,
        Some(preferences::UiThemePreference::Classic)
    );

    let updated = preferences::patch_user_preferences(
        &op,
        user_id,
        &json!({
            "color_mode": "dark",
            "primary_color": "blue",
            "content_width": "wide"
        }),
    )
    .await?;

    assert_eq!(updated.selected_space_id.as_deref(), Some("space-1"));
    assert_eq!(
        updated.color_mode,
        Some(preferences::ColorModePreference::Dark)
    );
    assert_eq!(
        updated.content_width,
        Some(preferences::ContentWidthPreference::Wide)
    );
    assert_eq!(
        updated.primary_color,
        Some(preferences::PrimaryColorPreference::Blue)
    );

    let user_hash = hex::encode(Sha256::digest(user_id.as_bytes()));
    let hashed_path = format!("users/{user_hash}/preferences.json");
    let raw_path = format!("users/{user_id}/preferences.json");
    assert!(op.exists(&hashed_path).await?);
    assert!(!op.exists(&raw_path).await?);

    let stored = preferences::get_user_preferences(&op, user_id).await?;
    assert_eq!(stored.selected_space_id.as_deref(), Some("space-1"));
    assert_eq!(stored.locale, Some(preferences::LocalePreference::Ja));
    assert_eq!(
        stored.color_mode,
        Some(preferences::ColorModePreference::Dark)
    );

    Ok(())
}

#[tokio::test]
async fn preference_patches_retry_the_authority_cas_without_lost_updates() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let user_id = "concurrent-preferences@example.com";
    let locale_patch = json!({"locale": "ja"});
    let theme_patch = json!({"ui_theme": "pop"});

    let (locale, theme) = tokio::join!(
        preferences::patch_user_preferences(&op, user_id, &locale_patch),
        preferences::patch_user_preferences(&op, user_id, &theme_patch),
    );
    locale?;
    theme?;

    let stored = preferences::get_user_preferences(&op, user_id).await?;
    assert_eq!(stored.locale, Some(preferences::LocalePreference::Ja));
    assert_eq!(stored.ui_theme, Some(preferences::UiThemePreference::Pop));
    Ok(())
}
