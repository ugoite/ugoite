use crate::index;
use crate::integrity::IntegrityProvider;
use crate::note;
use anyhow::{Context, Result};
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;

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
        "list".to_string(),
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
    let content = class_def.to_string().into_bytes();
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
    if strategies.is_none() {
        return Ok(0);
    }
    let strategies = strategies.unwrap();
    let strategies_obj = strategies
        .as_object()
        .context("Strategies must be an object")?;
    let class_name = class_def["name"].as_str().context("Class name required")?;

    let note_ids = note::list_notes(op, ws_path).await?;
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
