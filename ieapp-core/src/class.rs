use crate::index;
use crate::integrity::IntegrityProvider;
use crate::note;
use anyhow::{Context, Result};
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;

async fn migrate_legacy_schemas_dir(op: &Operator, ws_path: &str) -> Result<bool> {
    let legacy_path = format!("{}/schemas/", ws_path);
    if !op.exists(&legacy_path).await? {
        return Ok(false);
    }

    let classes_path = format!("{}/classes/", ws_path);
    if !op.exists(&classes_path).await? {
        op.create_dir(&classes_path).await?;
    }

    let mut migrated = false;
    let mut lister = op.lister(&legacy_path).await?;
    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() != EntryMode::FILE {
            continue;
        }

        let entry_name = entry.name();
        if !entry_name.ends_with(".json") {
            continue;
        }

        let entry_path = if entry_name.contains('/') {
            entry_name.to_string()
        } else {
            format!("{}{}", legacy_path, entry_name)
        };
        let file_name = entry_name.rsplit('/').next().unwrap_or(entry_name);
        let target_path = format!("{}{}", classes_path, file_name);

        if op.exists(&target_path).await? {
            continue;
        }

        let bytes = op.read(&entry_path).await?;
        op.write(&target_path, bytes).await?;
        op.delete(&entry_path).await?;
        migrated = true;
    }

    Ok(migrated)
}

pub async fn list_classes(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let classes_path = format!("{}/classes/", ws_path);
    if !op.exists(&classes_path).await? {
        let migrated = migrate_legacy_schemas_dir(op, ws_path).await?;
        if !migrated && !op.exists(&classes_path).await? {
            return Ok(vec![]);
        }
    }

    // List all files in classes/ directory
    let mut lister = op.lister(&classes_path).await?;
    let mut classes = Vec::new();

    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() == EntryMode::FILE && entry.name().ends_with(".json") {
            let entry_name = entry.name();
            let entry_path = if entry_name.contains('/') {
                entry_name.to_string()
            } else {
                format!("{}{}", classes_path, entry_name)
            };
            let bytes = op.read(&entry_path).await?;
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes.to_vec()) {
                classes.push(value);
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
        "list".to_string(),
        "markdown".to_string(),
    ])
}

pub async fn get_class(op: &Operator, ws_path: &str, class_name: &str) -> Result<Value> {
    migrate_legacy_schemas_dir(op, ws_path).await?;
    let class_path = format!("{}/classes/{}.json", ws_path, class_name);
    let bytes = op
        .read(&class_path)
        .await
        .context(format!("Class {} not found", class_name))?;
    let content: Value = serde_json::from_slice(&bytes.to_vec())?;
    Ok(content)
}

pub async fn upsert_class(op: &Operator, ws_path: &str, class_def: &Value) -> Result<()> {
    migrate_legacy_schemas_dir(op, ws_path).await?;
    let name = class_def["name"]
        .as_str()
        .context("Class definition missing 'name' field")?;

    let class_path = format!("{}/classes/{}.json", ws_path, name);
    let content = serde_json::to_vec_pretty(class_def)?;
    op.write(&class_path, content).await?;

    Ok(())
}

pub async fn migrate_class<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    class_def: &Value,
    strategies: Option<Value>,
    integrity: &I,
) -> Result<usize> {
    migrate_legacy_schemas_dir(op, ws_path).await?;
    if strategies.is_none() {
        return Ok(0);
    }
    let strategies = strategies.unwrap();
    let strategies_obj = strategies
        .as_object()
        .context("Strategies must be an object")?;
    let class_name = class_def["name"].as_str().context("Class name required")?;

    let note_entries = note::list_notes(op, ws_path).await?;
    let note_ids: Vec<String> = note_entries
        .iter()
        .filter_map(|val| {
            val.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let mut updated_count = 0;

    for note_id in note_ids {
        let note_content = match note::get_note_content(op, ws_path, &note_id).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let props = index::extract_properties(&note_content.markdown);
        if props.get("class").and_then(|v| v.as_str()) != Some(class_name) {
            continue;
        }

        let original_md = note_content.markdown.clone();
        let new_md = apply_migration(&original_md, strategies_obj);

        if new_md != original_md {
            note::update_note(
                op,
                ws_path,
                &note_id,
                &new_md,
                Some(&note_content.revision_id),
                "system-migration",
                None,
                integrity,
            )
            .await?;
            updated_count += 1;
        }
    }

    Ok(updated_count)
}

fn apply_migration(markdown: &str, strategies: &serde_json::Map<String, Value>) -> String {
    let header_re = Regex::new(r"^##\s+(.+)$").unwrap();

    struct Section {
        title: String,
        content: String,
    }

    let mut preamble_str = String::new();
    let mut sections: Vec<Section> = Vec::new();
    let mut buffer = Vec::new();
    let mut current_section: Option<String> = None;

    // Use loop over collected lines to handle parsing
    for line in markdown.lines() {
        if let Some(caps) = header_re.captures(line) {
            let title = caps.get(1).unwrap().as_str().trim().to_string();

            if let Some(curr_title) = current_section.take() {
                sections.push(Section {
                    title: curr_title,
                    content: buffer.join("\n"),
                });
            } else {
                preamble_str = buffer.join("\n");
            }
            buffer.clear();
            current_section = Some(title);
            continue;
        }
        buffer.push(line.to_string());
    }

    if let Some(curr_title) = current_section {
        sections.push(Section {
            title: curr_title,
            content: buffer.join("\n"),
        });
    } else {
        preamble_str = buffer.join("\n");
    }

    // Apply strategies
    let mut final_sections = Vec::new();
    let mut existing_titles = HashSet::new();

    for sec in &sections {
        existing_titles.insert(sec.title.clone());
    }

    for sec in sections {
        if let Some(strat) = strategies.get(&sec.title) {
            if strat.is_null() {
                continue;
            }
        }
        final_sections.push(sec);
    }

    for (field, strat) in strategies {
        if !existing_titles.contains(field) && strat.is_string() {
            final_sections.push(Section {
                title: field.clone(),
                content: strat.as_str().unwrap().to_string(),
            });
        }
    }

    // Reconstruct
    let mut res = preamble_str;
    if !res.is_empty() && !res.ends_with('\n') {
        res.push('\n');
    }

    for sec in final_sections {
        if !res.is_empty() && !res.ends_with("\n\n") {
            if res.ends_with('\n') {
                res.push('\n');
            } else {
                res.push_str("\n\n");
            }
        }
        res.push_str(&format!("## {}\n", sec.title));
        if !sec.content.is_empty() {
            res.push_str(&sec.content);
            res.push('\n');
        }
    }

    // Normalize newlines (reduce 3+ newlines to 2)
    let re_newlines = Regex::new(r"\n{3,}").unwrap();
    let normalized = re_newlines.replace_all(&res, "\n\n");

    normalized.trim_end().to_string()
}
