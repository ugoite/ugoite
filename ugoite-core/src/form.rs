use crate::entry;
use crate::iceberg_store;
use crate::integrity::IntegrityProvider;
use crate::metadata;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde_json::{Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

pub async fn list_forms(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let mut forms = Vec::new();
    for form_name in list_form_names(op, ws_path).await? {
        if let Ok(value) = read_form_definition(op, ws_path, &form_name).await {
            forms.push(enrich_form_definition(&value)?);
        }
    }
    Ok(forms)
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

pub async fn get_form(op: &Operator, ws_path: &str, form_name: &str) -> Result<Value> {
    let form_def = read_form_definition(op, ws_path, form_name).await?;
    enrich_form_definition(&form_def)
}

pub async fn upsert_form(op: &Operator, ws_path: &str, form_def: &Value) -> Result<()> {
    let normalized = normalize_form_definition(form_def)?;
    let form_name = normalized
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?;
    let existing = match iceberg_store::load_form_definition(op, ws_path, form_name).await {
        Ok(def) => Some(def),
        Err(_) => iceberg_store::load_form_definition_from_metadata(op, ws_path, form_name)
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
        let schema_fields = iceberg_store::load_form_schema_fields(op, ws_path, form_name)
            .await
            .ok()
            .flatten();
        let schema_mismatch = match schema_fields {
            Some(schema_fields) => schema_fields != normalized_fields,
            None => false,
        };
        if fields_changed || def_changed || schema_mismatch {
            rebuild_form_tables(op, ws_path, form_name, &existing_def, &normalized).await?;
            return Ok(());
        }
    }

    iceberg_store::ensure_form_tables(op, ws_path, &normalized).await?;
    Ok(())
}

pub(crate) async fn upsert_metadata_form(
    op: &Operator,
    ws_path: &str,
    form_def: &Value,
) -> Result<()> {
    let normalized = normalize_form_definition_with_options(form_def, true)?;
    iceberg_store::ensure_form_tables(op, ws_path, &normalized).await?;
    Ok(())
}

pub async fn migrate_form<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    form_def: &Value,
    strategies: Option<Value>,
    integrity: &I,
) -> Result<usize> {
    let normalized = normalize_form_definition(form_def)?;
    let form_name = normalized["name"].as_str().context("Form name required")?;
    let existing_def = match iceberg_store::load_form_definition(op, ws_path, form_name).await {
        Ok(def) => Some(def),
        Err(_) => iceberg_store::load_form_definition_from_metadata(op, ws_path, form_name)
            .await
            .ok()
            .flatten(),
    };

    if let Some(existing_def) = existing_def {
        let fields_changed = existing_def.get("fields") != normalized.get("fields");
        if fields_changed {
            rebuild_form_tables(op, ws_path, form_name, &existing_def, &normalized).await?;
        } else {
            upsert_form(op, ws_path, &normalized).await?;
        }
    } else {
        upsert_form(op, ws_path, &normalized).await?;
    }

    let strategies = match strategies {
        Some(value) => value,
        None => return Ok(0),
    };
    let strategies_obj = strategies
        .as_object()
        .context("Strategies must be an object")?;

    let entry_entries = entry::list_entries(op, ws_path).await?;
    let entry_ids: Vec<String> = entry_entries
        .iter()
        .filter_map(|val| {
            let entry_form = val.get("form").and_then(|v| v.as_str());
            if entry_form != Some(form_name) {
                return None;
            }
            val.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let mut updated_count = 0;

    let form_set: HashSet<String> = normalized
        .get("fields")
        .and_then(|v| v.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();

    for entry_id in entry_ids {
        let mut row = match entry::read_entry_row(op, ws_path, form_name, &entry_id).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mut fields = row.fields.as_object().cloned().unwrap_or_else(Map::new);
        let mut changed = false;

        for (field, strategy) in strategies_obj {
            if !form_set.contains(field) {
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

        let mut timestamp = entry::now_ts();
        if timestamp <= row.updated_at {
            timestamp = row.updated_at + 0.001;
        }
        let new_rev_id = Uuid::new_v4().to_string();

        row.parent_revision_id = Some(row.revision_id.clone());
        row.revision_id = new_rev_id.clone();
        row.updated_at = timestamp;
        row.fields = Value::Object(fields);
        row.author = "system-migration".to_string();

        let markdown = entry::render_markdown_for_form(
            &row.title,
            form_name,
            &row.tags,
            &row.fields,
            &row.extra_attributes,
            &normalized,
        );
        let checksum = integrity.checksum(&markdown);
        let signature = integrity.signature(&markdown);
        row.integrity = entry::IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        };

        entry::write_entry_row(op, ws_path, form_name, &entry_id, &row).await?;

        let revision = entry::RevisionRow {
            revision_id: new_rev_id.clone(),
            entry_id: entry_id.to_string(),
            parent_revision_id: row.parent_revision_id.clone(),
            timestamp,
            author: row.author.clone(),
            fields: row.fields.clone(),
            extra_attributes: row.extra_attributes.clone(),
            markdown_checksum: checksum,
            integrity: row.integrity.clone(),
            restored_from: None,
        };
        entry::append_revision_row_for_form(op, ws_path, form_name, &revision, &normalized).await?;

        updated_count += 1;
    }

    Ok(updated_count)
}

pub(crate) async fn list_form_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    iceberg_store::list_form_names(op, ws_path).await
}

pub(crate) async fn read_form_definition(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<Value> {
    iceberg_store::load_form_definition(op, ws_path, form_name)
        .await
        .context(format!("Form {} not found", form_name))
}

fn normalize_form_definition(form_def: &Value) -> Result<Value> {
    normalize_form_definition_with_options(form_def, false)
}

fn normalize_form_definition_with_options(
    form_def: &Value,
    allow_reserved_metadata_form: bool,
) -> Result<Value> {
    let name = form_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?;
    if !allow_reserved_metadata_form && is_reserved_metadata_form(name) {
        return Err(anyhow!(
            "Form name '{}' is reserved for metadata forms",
            name
        ));
    }
    let version = form_def
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    let fields = normalize_form_fields(form_def.get("fields"));
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
    let allow_extra_attributes = form_def
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

fn is_reserved_metadata_form(name: &str) -> bool {
    metadata::is_reserved_metadata_form(name)
}

fn normalize_form_fields(fields: Option<&Value>) -> Value {
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

fn enrich_form_definition(form_def: &Value) -> Result<Value> {
    let name = form_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?;
    let template = form_template_from_fields(name, form_def.get("fields"));

    let mut enriched = form_def.clone();
    if let Some(obj) = enriched.as_object_mut() {
        obj.insert("template".to_string(), Value::String(template));
    }
    Ok(enriched)
}

fn form_template_from_fields(form_name: &str, fields: Option<&Value>) -> String {
    let mut template = format!("# {}\n\n", form_name);
    if let Some(Value::Object(map)) = fields {
        let mut field_names: Vec<&String> = map.keys().collect();
        field_names.sort();
        for name in field_names {
            template.push_str(&format!("## {}\n\n", name));
        }
    }
    template
}

async fn rebuild_form_tables(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    existing_def: &Value,
    new_def: &Value,
) -> Result<()> {
    let entry_rows = entry::list_form_entry_rows(op, ws_path, form_name, existing_def).await?;
    let revision_rows =
        entry::list_form_revision_rows(op, ws_path, form_name, existing_def).await?;

    iceberg_store::drop_form_tables(op, ws_path, form_name).await?;
    iceberg_store::ensure_form_tables(op, ws_path, new_def).await?;

    for row in entry_rows {
        entry::write_entry_row(op, ws_path, form_name, &row.entry_id, &row).await?;
    }

    for rev in revision_rows {
        entry::append_revision_row_for_form(op, ws_path, form_name, &rev, new_def).await?;
    }

    Ok(())
}

// Migration logic handled via Iceberg row updates.
