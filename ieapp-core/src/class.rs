use crate::iceberg_store;
use crate::integrity::IntegrityProvider;
use crate::metadata;
use crate::note;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde_json::{Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

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
        "markdown".to_string(),
        "number".to_string(),
        "double".to_string(),
        "float".to_string(),
        "integer".to_string(),
        "long".to_string(),
        "boolean".to_string(),
        "date".to_string(),
        "time".to_string(),
        "timestamp".to_string(),
        "timestamp_tz".to_string(),
        "timestamp_ns".to_string(),
        "timestamp_tz_ns".to_string(),
        "uuid".to_string(),
        "binary".to_string(),
        "list".to_string(),
        "object_list".to_string(),
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
    let existing = match iceberg_store::load_class_definition(op, ws_path, class_name).await {
        Ok(def) => Some(def),
        Err(_) => iceberg_store::load_class_definition_from_metadata(op, ws_path, class_name)
            .await
            .ok()
            .flatten(),
    };
    if let Some(existing_def) = existing {
        let fields_changed = existing_def.get("fields") != normalized.get("fields");
        let def_changed =
            serde_json::to_string(&existing_def)? != serde_json::to_string(&normalized)?;
        let normalized_fields: HashSet<String> = normalized
            .get("fields")
            .and_then(|v| v.as_object())
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        let schema_fields = iceberg_store::load_class_schema_fields(op, ws_path, class_name)
            .await
            .ok()
            .flatten();
        let schema_mismatch = match schema_fields {
            Some(schema_fields) => schema_fields != normalized_fields,
            None => false,
        };
        if fields_changed || def_changed || schema_mismatch {
            rebuild_class_tables(op, ws_path, class_name, &existing_def, &normalized).await?;
            return Ok(());
        }
    }

    iceberg_store::ensure_class_tables(op, ws_path, &normalized).await?;
    Ok(())
}

pub(crate) async fn upsert_metadata_class(
    op: &Operator,
    ws_path: &str,
    class_def: &Value,
) -> Result<()> {
    let normalized = normalize_class_definition_with_options(class_def, true)?;
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
    let existing_def = match iceberg_store::load_class_definition(op, ws_path, class_name).await {
        Ok(def) => Some(def),
        Err(_) => iceberg_store::load_class_definition_from_metadata(op, ws_path, class_name)
            .await
            .ok()
            .flatten(),
    };

    if let Some(existing_def) = existing_def {
        let fields_changed = existing_def.get("fields") != normalized.get("fields");
        if fields_changed {
            rebuild_class_tables(op, ws_path, class_name, &existing_def, &normalized).await?;
        } else {
            upsert_class(op, ws_path, &normalized).await?;
        }
    } else {
        upsert_class(op, ws_path, &normalized).await?;
    }

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

    let class_set: HashSet<String> = normalized
        .get("fields")
        .and_then(|v| v.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();

    for note_id in note_ids {
        let mut row = match note::read_note_row(op, ws_path, class_name, &note_id).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mut fields = row.fields.as_object().cloned().unwrap_or_else(Map::new);
        let mut changed = false;

        for (field, strategy) in strategies_obj {
            if !class_set.contains(field) {
                continue;
            }
            if strategy.is_null() {
                if fields.remove(field).is_some() {
                    changed = true;
                }
                continue;
            }

            let updated = match fields.get(field) {
                Some(existing) => existing != strategy,
                None => true,
            };
            if updated {
                fields.insert(field.clone(), strategy.clone());
                changed = true;
            }
        }

        if !changed {
            continue;
        }

        let mut timestamp = note::now_ts();
        if timestamp <= row.updated_at {
            timestamp = row.updated_at + 0.001;
        }
        let new_rev_id = Uuid::new_v4().to_string();

        row.parent_revision_id = Some(row.revision_id.clone());
        row.revision_id = new_rev_id.clone();
        row.updated_at = timestamp;
        row.fields = Value::Object(fields);
        row.author = "system-migration".to_string();

        let markdown = note::render_markdown_for_class(
            &row.title,
            class_name,
            &row.tags,
            &row.fields,
            &row.extra_attributes,
            &normalized,
        );
        let checksum = integrity.checksum(&markdown);
        let signature = integrity.signature(&markdown);
        row.integrity = note::IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        };

        note::write_note_row(op, ws_path, class_name, &note_id, &row).await?;

        let revision = note::RevisionRow {
            revision_id: new_rev_id.clone(),
            note_id: note_id.to_string(),
            parent_revision_id: row.parent_revision_id.clone(),
            timestamp,
            author: row.author.clone(),
            fields: row.fields.clone(),
            extra_attributes: row.extra_attributes.clone(),
            markdown_checksum: checksum,
            integrity: row.integrity.clone(),
            restored_from: None,
        };
        note::append_revision_row_for_class(op, ws_path, class_name, &revision, &normalized)
            .await?;

        updated_count += 1;
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
    normalize_class_definition_with_options(class_def, false)
}

fn normalize_class_definition_with_options(
    class_def: &Value,
    allow_reserved_metadata_class: bool,
) -> Result<Value> {
    let name = class_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Class definition missing 'name' field")?;
    if !allow_reserved_metadata_class && is_reserved_metadata_class(name) {
        return Err(anyhow!(
            "Class name '{}' is reserved for metadata classes",
            name
        ));
    }
    let version = class_def
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let fields = normalize_class_fields(class_def.get("fields"));
    if let Some(field_map) = fields.as_object() {
        for name in field_map.keys() {
            if is_reserved_metadata_column(name) {
                return Err(anyhow!(
                    "Field name '{}' is reserved for metadata columns",
                    name
                ));
            }
        }
    }
    let allow_extra_attributes = class_def
        .get("allow_extra_attributes")
        .and_then(|v| v.as_str())
        .unwrap_or("deny");
    if !matches!(
        allow_extra_attributes,
        "deny" | "allow_json" | "allow_columns"
    ) {
        return Err(anyhow!(
            "Invalid allow_extra_attributes value: {}",
            allow_extra_attributes
        ));
    }

    Ok(serde_json::json!({
        "name": name,
        "version": version,
        "fields": fields,
        "allow_extra_attributes": allow_extra_attributes,
    }))
}

fn is_reserved_metadata_column(name: &str) -> bool {
    metadata::is_reserved_metadata_column(name)
}

fn is_reserved_metadata_class(name: &str) -> bool {
    metadata::is_reserved_metadata_class(name)
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

// Migration logic handled via Iceberg row updates.
