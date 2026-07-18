use crate::entry;
use crate::error::AppError;
use crate::iceberg_store;
use crate::integrity::IntegrityProvider;
use crate::metadata;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use ugoite_domain::form::{
    FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
};
use ugoite_domain::id::validate_form_name;
use ugoite_domain::id::{FieldId, FormId};
use uuid::Uuid;

const EXTRA_ATTRIBUTES_POLICY_METADATA: &str = "ugoite.extra_attributes_policy";

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
        "sql".to_string(),
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
        "row_reference".to_string(),
        "binary".to_string(),
        "list".to_string(),
        "object_list".to_string(),
    ])
}

pub async fn get_form(op: &Operator, ws_path: &str, form_name: &str) -> Result<Value> {
    validate_form_path_segment(form_name)?;
    let form_def = read_form_definition(op, ws_path, form_name).await?;
    enrich_form_definition(&form_def)
}

pub async fn upsert_form(op: &Operator, ws_path: &str, form_def: &Value) -> Result<()> {
    let mut normalized = normalize_form_definition(form_def)?;
    let form_name = normalized
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?
        .to_string();
    validate_row_reference_targets(op, ws_path, &form_name, &normalized).await?;
    let existing = iceberg_store::load_form_definition(op, ws_path, &form_name)
        .await
        .ok();
    if let Some(existing_def) = existing {
        preserve_stable_identities(&mut normalized, &existing_def)?;
        let current_domain = to_domain_form(&existing_def)?;
        let desired_domain = to_domain_form(&normalized)?;
        let changes = form_changes(&current_domain, &desired_domain)?;
        if !changes.is_empty() {
            let workspace = iceberg_store::native_workspace(op, ws_path).await?;
            workspace
                .evolve_form(&FormChangeSet {
                    form_id: current_domain.id,
                    expected_version: Some(current_domain.version),
                    changes,
                })
                .await?;
        }
        return Ok(());
    }

    iceberg_store::ensure_form_tables(op, ws_path, &normalized).await?;
    Ok(())
}

fn form_changes(current: &FormDefinition, desired: &FormDefinition) -> Result<Vec<FormChange>> {
    let mut changes = Vec::new();
    if current.name != desired.name {
        changes.push(FormChange::RenameForm {
            name: desired.name.clone(),
        });
    }
    if current.description != desired.description
        || current.allow_extra_attributes != desired.allow_extra_attributes
    {
        changes.push(FormChange::UpdateFormMetadata {
            description: desired.description.clone(),
            allow_extra_attributes: desired.allow_extra_attributes,
            extension_metadata: desired.extension_metadata.clone(),
        });
    }
    for field in &desired.fields {
        let Some(previous) = current.fields.iter().find(|value| value.id == field.id) else {
            changes.push(FormChange::AddField(field.clone()));
            continue;
        };
        if previous.name != field.name {
            changes.push(FormChange::RenameField {
                field_id: field.id,
                name: field.name.clone(),
            });
        }
        if previous.field_type != field.field_type {
            changes.push(FormChange::ChangeFieldType {
                field_id: field.id,
                field_type: field.field_type.clone(),
            });
        }
        if previous.required != field.required {
            changes.push(FormChange::ChangeRequired {
                field_id: field.id,
                required: field.required,
            });
        }
        if previous.deprecated != field.deprecated {
            changes.push(if field.deprecated {
                FormChange::DeprecateField { field_id: field.id }
            } else {
                FormChange::RestoreField { field_id: field.id }
            });
        }
        if previous.label != field.label
            || previous.description != field.description
            || previous.semantic_role != field.semantic_role
            || previous.validation != field.validation
            || previous.enum_values != field.enum_values
        {
            changes.push(FormChange::UpdateFieldMetadata {
                field_id: field.id,
                label: field.label.clone(),
                description: field.description.clone(),
                semantic_role: field.semantic_role.clone(),
                validation: field.validation.clone(),
                enum_values: field.enum_values.clone(),
            });
        }
    }
    if current
        .fields
        .iter()
        .any(|field| !desired.fields.iter().any(|value| value.id == field.id))
    {
        return Err(anyhow!("physical Form field removal is not supported"));
    }
    Ok(changes)
}

pub(crate) async fn upsert_metadata_form(
    op: &Operator,
    ws_path: &str,
    form_def: &Value,
) -> Result<()> {
    let mut normalized = normalize_form_definition_with_options(form_def, true)?;
    if let Some(form_name) = normalized
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        if let Ok(existing) = iceberg_store::load_form_definition(op, ws_path, &form_name).await {
            preserve_stable_identities(&mut normalized, &existing)?;
            let current_domain = to_domain_form(&existing)?;
            let desired_domain = to_domain_form(&normalized)?;
            if form_changes(&current_domain, &desired_domain)?.is_empty() {
                return Ok(());
            }
        }
    }
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
    validate_row_reference_targets(op, ws_path, form_name, &normalized).await?;
    let existing_def = iceberg_store::load_form_definition(op, ws_path, form_name)
        .await
        .ok();

    if let Some(existing_def) = existing_def {
        let fields_changed = existing_def.get("fields") != normalized.get("fields");
        if fields_changed {
            return Err(anyhow!("FormChangeSet schema changes require an explicit migration plan; destructive table rebuild is disabled"));
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
        row.entry_version = row.entry_version.saturating_add(1);
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
            state: Some(row.clone()),
            entry_version: row.entry_version,
            operation: "upsert".to_string(),
            source_kind: "migration".to_string(),
            source_id: None,
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

pub(crate) fn to_domain_form(form_def: &Value) -> Result<FormDefinition> {
    let id = FormId::from(Uuid::parse_str(
        form_def
            .get("id")
            .and_then(Value::as_str)
            .context("Form definition missing stable id")?,
    )?);
    let version =
        FormVersion::new(form_def.get("version").and_then(Value::as_u64).unwrap_or(1) as u32)?;
    let name = form_def
        .get("name")
        .and_then(Value::as_str)
        .context("Form definition missing name")?
        .to_string();
    let mut fields = Vec::new();
    if let Some(field_map) = form_def.get("fields").and_then(Value::as_object) {
        for (name, definition) in field_map {
            let field_id = FieldId::new(
                definition
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .context("Form field missing stable id")?,
            )?;
            let field_type = domain_field_type(
                definition
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("string"),
            )?;
            fields.push(FormField {
                id: field_id,
                name: name.clone(),
                field_type,
                required: definition
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                label: definition
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                description: definition
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                semantic_role: definition
                    .get("semantic_role")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                reference_form: definition
                    .get("target_form")
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .map(FormId::from),
                validation: definition.get("validation").cloned(),
                enum_values: definition
                    .get("enum_values")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                deprecated: definition
                    .get("deprecated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    let mut extension_metadata = BTreeMap::new();
    if let Some(policy) = form_def
        .get("allow_extra_attributes")
        .and_then(Value::as_str)
    {
        extension_metadata.insert(
            EXTRA_ATTRIBUTES_POLICY_METADATA.to_string(),
            Value::String(policy.to_string()),
        );
    }
    let form = FormDefinition {
        id,
        version,
        name,
        description: form_def
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        fields,
        allow_extra_attributes: form_def
            .get("allow_extra_attributes")
            .and_then(Value::as_str)
            .is_some_and(|value| value != "deny"),
        extension_metadata,
    };
    form.validate()?;
    Ok(form)
}

fn domain_field_type(value: &str) -> Result<FieldType> {
    Ok(match value {
        "text" => FieldType::String,
        "string" => FieldType::String,
        "markdown" => FieldType::Markdown,
        "sql" => FieldType::Sql,
        "boolean" => FieldType::Boolean,
        "integer" => FieldType::Integer,
        "long" => FieldType::Long,
        "float" => FieldType::Float,
        "number" | "double" => FieldType::Double,
        "date" => FieldType::Date,
        "time" => FieldType::Time,
        "timestamp" => FieldType::Timestamp,
        "timestamp_tz" => FieldType::TimestampTz,
        "timestamp_ns" => FieldType::TimestampNs,
        "timestamp_tz_ns" => FieldType::TimestampTzNs,
        "uuid" => FieldType::Uuid,
        "binary" => FieldType::Binary,
        "list" => FieldType::List,
        "object_list" => FieldType::ObjectList,
        "row_reference" => FieldType::RowReference,
        other => return Err(anyhow!("unsupported Form field type: {other}")),
    })
}

pub(crate) fn from_domain_form(form: &FormDefinition) -> Value {
    let mut fields = Map::new();
    for field in &form.fields {
        fields.insert(
            field.name.clone(),
            serde_json::json!({
                "id": field.id.get(),
                "type": domain_field_type_name(&field.field_type),
                "required": field.required,
                "label": field.label,
                "description": field.description,
                "semantic_role": field.semantic_role,
                "deprecated": field.deprecated,
            }),
        );
    }
    serde_json::json!({
        "id": form.id.to_string(),
        "name": form.name,
        "version": form.version.get(),
        "description": form.description,
        "fields": fields,
        "allow_extra_attributes": form
            .extension_metadata
            .get(EXTRA_ATTRIBUTES_POLICY_METADATA)
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "allow_json" | "allow_columns" | "deny"))
            .unwrap_or(if form.allow_extra_attributes { "allow_json" } else { "deny" }),
    })
}

fn domain_field_type_name(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String => "string",
        FieldType::Markdown => "markdown",
        FieldType::Sql => "sql",
        FieldType::Boolean => "boolean",
        FieldType::Integer => "integer",
        FieldType::Long => "long",
        FieldType::Float => "float",
        FieldType::Double => "double",
        FieldType::Date => "date",
        FieldType::Time => "time",
        FieldType::Timestamp => "timestamp",
        FieldType::TimestampTz => "timestamp_tz",
        FieldType::TimestampNs => "timestamp_ns",
        FieldType::TimestampTzNs => "timestamp_tz_ns",
        FieldType::Uuid => "uuid",
        FieldType::Binary => "binary",
        FieldType::List => "list",
        FieldType::ObjectList => "object_list",
        FieldType::RowReference => "row_reference",
    }
}

fn normalize_form_definition_with_options(
    form_def: &Value,
    allow_reserved_metadata_form: bool,
) -> Result<Value> {
    let name = form_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?;
    validate_form_path_segment(name)?;
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
    let form_id = form_def
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    if let Some(field_map) = fields.as_object() {
        for name in field_map.keys() {
            if is_reserved_metadata_column(name) {
                return Err(anyhow!(
                    "Field name '{}' is reserved for metadata columns",
                    name
                ));
            }
        }
        validate_row_reference_field_defs(field_map)?;
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
        "id": form_id,
        "name": name,
        "version": version,
        "fields": fields,
        "allow_extra_attributes": allow_extra_attributes,
    }))
}

fn validate_row_reference_field_defs(field_map: &Map<String, Value>) -> Result<()> {
    for (name, def) in field_map {
        let field_type = def.get("type").and_then(|v| v.as_str()).unwrap_or("string");
        if field_type != "row_reference" {
            continue;
        }
        let target_form = def
            .get("target_form")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("row_reference field '{}' requires target_form", name))?;
        validate_form_path_segment(target_form)?;
        if is_reserved_metadata_form(target_form) {
            return Err(anyhow!(
                "row_reference field '{}' target_form '{}' is reserved",
                name,
                target_form
            ));
        }
    }
    Ok(())
}

async fn validate_row_reference_targets(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    form_def: &Value,
) -> Result<()> {
    let Some(field_map) = form_def.get("fields").and_then(|v| v.as_object()) else {
        return Ok(());
    };

    let mut available: HashSet<String> = list_form_names(op, ws_path)
        .await
        .with_context(|| format!("failed to list forms for workspace '{}'", ws_path))?
        .into_iter()
        .collect();
    available.insert(form_name.to_string());

    for (name, def) in field_map {
        let field_type = def.get("type").and_then(|v| v.as_str()).unwrap_or("string");
        if field_type != "row_reference" {
            continue;
        }
        let target_form = def
            .get("target_form")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow!("row_reference field '{}' requires target_form", name))?;
        validate_form_path_segment(target_form)?;
        if !available.contains(target_form) {
            return Err(anyhow!(
                "row_reference field '{}' target_form '{}' not found",
                name,
                target_form
            ));
        }
    }

    Ok(())
}

fn validate_form_path_segment(name: &str) -> Result<()> {
    validate_form_name(name).map_err(|error| AppError::invalid_identifier(error.to_string()).into())
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
            for (index, (name, def)) in map.iter().enumerate() {
                let mut def = def.clone();
                if let Some(object) = def.as_object_mut() {
                    // `required: false` is the default schema meaning. Do not
                    // let an explicit default in an API payload look like a
                    // Form evolution when the persisted definition omitted it.
                    if object.get("required").and_then(Value::as_bool) == Some(false) {
                        object.remove("required");
                    }
                    object.entry("id".to_string()).or_insert_with(|| {
                        Value::from(100 + i64::try_from(index).unwrap_or_default())
                    });
                }
                normalized.insert(name.clone(), def);
            }
        }
        Some(Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let mut def = item.clone();
                if let Some(obj) = def.as_object_mut() {
                    obj.remove("name");
                    if obj.get("required").and_then(Value::as_bool) == Some(false) {
                        obj.remove("required");
                    }
                    obj.entry("id".to_string()).or_insert_with(|| {
                        Value::from(100 + i64::try_from(index).unwrap_or_default())
                    });
                }
                normalized.insert(name.to_string(), def);
            }
        }
        _ => {}
    }

    Value::Object(normalized)
}

fn preserve_stable_identities(normalized: &mut Value, existing: &Value) -> Result<()> {
    let existing_id = existing
        .get("id")
        .and_then(Value::as_str)
        .context("stored Form is missing stable id")?;
    normalized
        .as_object_mut()
        .context("normalized Form must be an object")?
        .insert("id".to_string(), Value::String(existing_id.to_string()));
    let existing_fields = existing.get("fields").and_then(Value::as_object);
    if let (Some(fields), Some(existing_fields)) = (
        normalized.get_mut("fields").and_then(Value::as_object_mut),
        existing_fields,
    ) {
        for (name, field) in fields {
            if let Some(id) = existing_fields
                .get(name)
                .and_then(|value| value.get("id"))
                .cloned()
            {
                if let Some(object) = field.as_object_mut() {
                    object.insert("id".to_string(), id);
                }
            }
        }
    }
    Ok(())
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
