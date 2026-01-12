use anyhow::{Context, Result};
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};

pub async fn list_classes(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    let classes_path = format!("{}/classes/", ws_path);
    if !op.exists(&classes_path).await? {
        return Ok(vec![]);
    }

    // List all files in classes/ directory
    let mut lister = op.lister(&classes_path).await?;
    let mut classes = Vec::new();

    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() == EntryMode::FILE && entry.name().ends_with(".json") {
            let name = entry
                .name()
                .trim_end_matches(".json")
                .split('/')
                .next_back()
                .unwrap_or("");
            if !name.is_empty() {
                classes.push(name.to_string());
            }
        }
    }

    Ok(classes)
}

pub async fn list_column_types() -> Result<Vec<String>> {
    Ok(vec![
        "string".to_string(),
        "number".to_string(),
        "date".to_string(),
        "boolean".to_string(),
        "markdown".to_string(),
    ])
}

pub async fn get_class(op: &Operator, ws_path: &str, class_name: &str) -> Result<String> {
    let class_path = format!("{}/classes/{}.json", ws_path, class_name);
    let bytes = op
        .read(&class_path)
        .await
        .context(format!("Class {} not found", class_name))?;
    let content = String::from_utf8(bytes.to_vec())?;
    Ok(content)
}

pub async fn upsert_class(op: &Operator, ws_path: &str, class_def: &str) -> Result<()> {
    // Basic validation could happen here
    let parsed: serde_json::Value = serde_json::from_str(class_def)?;
    let name = parsed["name"]
        .as_str()
        .context("Class definition missing 'name' field")?;

    let class_path = format!("{}/classes/{}.json", ws_path, name);
    // write takes bytes, need to clone to owned vec if lifetime issue persists,
    // or ensure op.write handles it. op.write takes `impl Into<Buffer>`.
    // `&[u8]` implements `Into<Buffer>`.
    // The issue is async function captures references.
    let content = class_def.to_string().into_bytes();
    op.write(&class_path, content).await?;

    Ok(())
}

pub async fn migrate_class(
    _op: &Operator,
    _ws_path: &str,
    _new_class: &str,
    _strategies: Option<serde_json::Value>,
) -> Result<usize> {
    // TODO: Implement migration logic
    Ok(0)
}
