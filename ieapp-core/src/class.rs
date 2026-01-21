use crate::iceberg_store;
use crate::integrity::IntegrityProvider;
use crate::note;
use anyhow::{Context, Result};
use opendal::Operator;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub async fn list_classes(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let mut classes = Vec::new();
    for class_name in list_class_names(op, ws_path).await? {
        if let Ok(value) = read_class_definition(op, ws_path, &class_name).await {
            classes.push(enrich_class_definition(&value)?);
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
    let class_def = read_class_definition(op, ws_path, class_name).await?;
    enrich_class_definition(&class_def)
}

pub async fn upsert_class(op: &Operator, ws_path: &str, class_def: &Value) -> Result<()> {
    let normalized = normalize_class_definition(class_def)?;
    let class_name = normalized
        .get("name")
        .and_then(|v| v.as_str())
        .context("Class definition missing 'name' field")?;
    let existing = iceberg_store::load_class_definition(op, ws_path, class_name)
        .await
        .ok();
    if let Some(existing_def) = existing {
        let fields_changed = existing_def.get("fields") != normalized.get("fields");
        if fields_changed {
            rebuild_class_tables(op, ws_path, class_name, &existing_def, &normalized).await?;
            return Ok(());
        }
    }

    iceberg_store::ensure_class_tables(op, ws_path, &normalized).await?;
    Ok(())
}

pub async fn migrate_class<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    class_def: &Value,
    strategies: Option<Value>,
    integrity: &I,
) -> Result<usize> {
    let normalized = normalize_class_definition(class_def)?;
    let class_name = normalized["name"].as_str().context("Class name required")?;
    upsert_class(op, ws_path, &normalized).await?;

    let strategies = match strategies {
        Some(value) => value,
        None => return Ok(0),
    };
    let strategies_obj = strategies
        .as_object()
        .context("Strategies must be an object")?;

    let note_entries = note::list_notes(op, ws_path).await?;
    let note_ids: Vec<String> = note_entries
        .iter()
        .filter_map(|val| {
            let note_class = val.get("class").and_then(|v| v.as_str());
            if note_class != Some(class_name) {
                return None;
            }
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

pub(crate) async fn list_class_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    iceberg_store::list_class_names(op, ws_path).await
}

pub(crate) async fn read_class_definition(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<Value> {
    iceberg_store::load_class_definition(op, ws_path, class_name)
        .await
        .context(format!("Class {} not found", class_name))
}

fn normalize_class_definition(class_def: &Value) -> Result<Value> {
    let name = class_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Class definition missing 'name' field")?;
    let version = class_def
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let fields = normalize_class_fields(class_def.get("fields"));

    Ok(serde_json::json!({
        "name": name,
        "version": version,
        "fields": fields,
    }))
}

fn normalize_class_fields(fields: Option<&Value>) -> Value {
    let mut normalized = Map::new();

    match fields {
        Some(Value::Object(map)) => {
            for (name, def) in map {
                normalized.insert(name.clone(), def.clone());
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let mut def = item.clone();
                if let Some(obj) = def.as_object_mut() {
                    obj.remove("name");
                }
                normalized.insert(name.to_string(), def);
            }
        }
        _ => {}
    }

    Value::Object(normalized)
}

fn enrich_class_definition(class_def: &Value) -> Result<Value> {
    let name = class_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Class definition missing 'name' field")?;
    let template = class_template_from_fields(name, class_def.get("fields"));

    let mut enriched = class_def.clone();
    if let Some(obj) = enriched.as_object_mut() {
        obj.insert("template".to_string(), Value::String(template));
    }
    Ok(enriched)
}

fn class_template_from_fields(class_name: &str, fields: Option<&Value>) -> String {
    let mut template = format!("# {}\n\n", class_name);
    if let Some(Value::Object(map)) = fields {
        let mut field_names: Vec<&String> = map.keys().collect();
        field_names.sort();
        for name in field_names {
            template.push_str(&format!("## {}\n\n", name));
        }
    }
    template
}

async fn rebuild_class_tables(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    existing_def: &Value,
    new_def: &Value,
) -> Result<()> {
    let note_rows = note::list_class_note_rows(op, ws_path, class_name, existing_def).await?;
    let revision_rows =
        note::list_class_revision_rows(op, ws_path, class_name, existing_def).await?;

    iceberg_store::drop_class_tables(op, ws_path, class_name).await?;
    iceberg_store::ensure_class_tables(op, ws_path, new_def).await?;

    for row in note_rows {
        note::write_note_row(op, ws_path, class_name, &row.note_id, &row).await?;
    }

    for rev in revision_rows {
        note::append_revision_row_for_class(op, ws_path, class_name, &rev, new_def).await?;
    }

    Ok(())
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
