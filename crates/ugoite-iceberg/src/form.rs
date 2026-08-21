use crate::iceberg_store;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::metadata;
use ugoite_domain::form::{sql_column_name, sql_relation_name};
use ugoite_domain::form::{
    FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
    ListItemDefinition,
};
use ugoite_domain::id::validate_form_name;
use ugoite_domain::id::{FieldId, FormId};
use uuid::Uuid;

const EXTRA_ATTRIBUTES_POLICY_METADATA: &str = "ugoite.extra_attributes_policy";

fn invalid_form_input(message: impl Into<String>) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

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
        "asset_reference".to_string(),
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
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    let form_name = form_def
        .get("name")
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_form_input("Form definition missing 'name' field"))?
        .to_string();
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let known_forms = workspace.list_forms().await?;
    let existing_domain = known_forms
        .iter()
        .find(|form| form.name == form_name)
        .cloned();
    let mut normalized = normalize_form_definition(form_def)?;
    if let Some(existing_domain) = &existing_domain {
        // Resolve the persisted identity before translating authoring names
        // into target UUIDs. In particular, a self-reference must never point
        // at normalize_form_definition's transient UUID.
        let existing_def = from_domain_form(existing_domain);
        preserve_stable_identities(&mut normalized, &existing_def)?;
    }
    validate_row_reference_targets(&form_name, &mut normalized, &known_forms)?;
    if let Some(current_domain) = existing_domain {
        let desired_domain =
            to_domain_form(&normalized).map_err(|error| invalid_form_input(error.to_string()))?;
        let changes = form_changes(&current_domain, &desired_domain)?;
        if !changes.is_empty() {
            let command = crate::publication_context(
                format!(
                    "form-evolve:{}:{}",
                    current_domain.id,
                    current_domain.version.get()
                ),
                "form.evolve",
                &changes,
            )?;
            crate::authorization::ensure_authorization_write_fence().await?;
            workspace
                .commit(command)?
                .evolve_form(&FormChangeSet {
                    form_id: current_domain.id,
                    expected_version: Some(current_domain.version),
                    changes,
                })
                .await?;
        }
        return Ok(());
    }

    // Validate the authoring payload before entering the storage mutation path.
    // This keeps malformed user input typed while leaving storage failures internal.
    to_domain_form(&normalized).map_err(|error| invalid_form_input(error.to_string()))?;
    crate::authorization::ensure_authorization_write_fence().await?;
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
        if previous.field_type == FieldType::List && field.field_type == FieldType::List {
            let previous_item_type = previous
                .list_item
                .as_ref()
                .map(|item| item.field_type.as_str())
                .unwrap_or("string");
            let desired_item_type = field
                .list_item
                .as_ref()
                .map(|item| item.field_type.as_str())
                .unwrap_or("string");
            if previous_item_type != desired_item_type {
                return Err(AppError::form_field_type_change_not_supported(
                    &previous.name,
                    previous_item_type,
                    desired_item_type,
                )
                .into());
            }
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
            || previous.reference_form != field.reference_form
            || previous.list_item != field.list_item
            || previous.validation != field.validation
            || previous.enum_values != field.enum_values
        {
            changes.push(FormChange::UpdateFieldMetadata {
                field_id: field.id,
                label: field.label.clone(),
                description: field.description.clone(),
                semantic_role: field.semantic_role.clone(),
                reference_form: field.reference_form,
                list_item: field.list_item.clone(),
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
        let removed = current
            .fields
            .iter()
            .filter(|field| !desired.fields.iter().any(|value| value.id == field.id))
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>();
        return Err(AppError::form_field_removal_not_supported(removed.join(", ")).into());
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
            let list_item = definition
                .get("items")
                .filter(|items| !items.is_null())
                .map(|items| -> Result<ListItemDefinition> {
                    let items = items
                        .as_object()
                        .context("Form list items must be an object")?;
                    let item_type = items
                        .get("type")
                        .and_then(Value::as_str)
                        .context("Form list items missing type")?;
                    Ok(ListItemDefinition {
                        field_type: domain_field_type(item_type)?,
                        reference_form: items
                            .get("target_form")
                            .and_then(Value::as_str)
                            .and_then(|value| Uuid::parse_str(value).ok())
                            .map(FormId::from),
                    })
                })
                .transpose()?;
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
                list_item,
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
        "asset_reference" => FieldType::AssetReference,
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
                "target_form": field.reference_form.map(|value| value.to_string()),
                "items": field.list_item.as_ref().map(|item| serde_json::json!({
                    "type": domain_field_type_name(&item.field_type),
                    "target_form": item.reference_form.map(|value| value.to_string()),
                })),
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
    field_type.as_str()
}

fn normalize_form_definition_with_options(
    form_def: &Value,
    allow_reserved_metadata_form: bool,
) -> Result<Value> {
    let name = form_def
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid_form_input("Form definition missing 'name' field"))?;
    validate_form_path_segment(name)?;
    if !allow_reserved_metadata_form && is_reserved_metadata_form(name) {
        return Err(invalid_form_input(format!(
            "Form name '{}' is reserved for metadata forms",
            name
        )));
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
                return Err(invalid_form_input(format!(
                    "Field name '{}' is reserved for metadata columns",
                    name
                )));
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
        return Err(invalid_form_input(format!(
            "Invalid allow_extra_attributes value: {}",
            allow_extra_attributes
        )));
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
        let target = if field_type == "row_reference" {
            def.get("target_form")
        } else if field_type == "list"
            && def
                .get("items")
                .and_then(|items| items.get("type"))
                .and_then(Value::as_str)
                == Some("row_reference")
        {
            def.get("items").and_then(|items| items.get("target_form"))
        } else {
            None
        };
        if let Some(target_form) = target.and_then(Value::as_str) {
            let target_form = target_form.trim();
            if target_form.is_empty() {
                return Err(invalid_form_input(format!(
                    "reference field '{}' requires target_form",
                    name
                )));
            }
            if Uuid::parse_str(target_form).is_err() {
                validate_form_path_segment(target_form)?;
                if is_reserved_metadata_form(target_form) {
                    return Err(invalid_form_input(format!(
                        "reference field '{}' target_form '{}' is reserved",
                        name, target_form
                    )));
                }
            }
        } else if field_type == "row_reference"
            || (field_type == "list"
                && def
                    .get("items")
                    .and_then(|items| items.get("type"))
                    .and_then(Value::as_str)
                    == Some("row_reference"))
        {
            return Err(invalid_form_input(format!(
                "reference field '{}' requires target_form",
                name
            )));
        }
    }
    Ok(())
}

fn validate_row_reference_targets(
    form_name: &str,
    form_def: &mut Value,
    known_forms: &[FormDefinition],
) -> Result<()> {
    let Some(field_map) = form_def.get("fields").and_then(|v| v.as_object()) else {
        return Ok(());
    };

    let mut available = known_forms
        .iter()
        .map(|form| (form.name.clone(), form.id.to_string()))
        .collect::<HashMap<_, _>>();
    if let Some(id) = form_def.get("id").and_then(Value::as_str) {
        available.insert(form_name.to_string(), id.to_string());
    }

    for (name, def) in field_map.clone() {
        let field_type = def.get("type").and_then(|v| v.as_str()).unwrap_or("string");
        let (target, container) = if field_type == "row_reference" {
            (def.get("target_form").and_then(Value::as_str), None)
        } else if field_type == "list"
            && def
                .get("items")
                .and_then(|items| items.get("type"))
                .and_then(Value::as_str)
                == Some("row_reference")
        {
            (
                def.get("items")
                    .and_then(|items| items.get("target_form").and_then(Value::as_str)),
                Some("items"),
            )
        } else {
            continue;
        };
        let Some(target_form) = target.map(str::trim) else {
            return Err(invalid_form_input(format!(
                "reference field '{}' requires target_form",
                name
            )));
        };
        let target_id = if let Ok(id) = Uuid::parse_str(target_form) {
            if !known_forms.iter().any(|form| form.id == FormId::from(id))
                && form_def.get("id").and_then(Value::as_str) != Some(target_form)
            {
                return Err(invalid_form_input(format!(
                    "reference field '{}' target_form '{}' not found",
                    name, target_form
                )));
            }
            id.to_string()
        } else {
            let Some(id) = available.get(target_form) else {
                return Err(invalid_form_input(format!(
                    "reference field '{}' target_form '{}' not found",
                    name, target_form
                )));
            };
            id.clone()
        };
        let field = form_def
            .get_mut("fields")
            .and_then(Value::as_object_mut)
            .and_then(|fields| fields.get_mut(&name))
            .context("normalized Form field disappeared")?;
        if let Some(container) = container {
            field
                .get_mut(container)
                .and_then(Value::as_object_mut)
                .context("list item definition is not an object")?
                .insert("target_form".to_string(), Value::String(target_id));
        } else {
            field
                .as_object_mut()
                .context("Form field is not an object")?
                .insert("target_form".to_string(), Value::String(target_id));
        }
        if field_type != "row_reference" {
            continue;
        }
        if target_form.is_empty() {
            return Err(invalid_form_input(format!(
                "reference field '{}' requires target_form",
                name
            )));
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
        let mut used_ids = existing_fields
            .values()
            .filter_map(|field| field.get("id").and_then(Value::as_i64))
            .collect::<BTreeSet<_>>();
        let mut stable_names = HashSet::new();

        // A field keeps its physical identity only when its authoring name is
        // unchanged. Never infer a rename from position or from a provisional
        // ID assigned to an incoming field.
        for (name, field) in fields.iter_mut() {
            if let Some(id) = existing_fields
                .get(name)
                .and_then(|value| value.get("id"))
                .cloned()
            {
                if let Some(object) = field.as_object_mut() {
                    object.insert("id".to_string(), id);
                    stable_names.insert(name.clone());
                }
            }
        }

        // Provisional IDs are position-derived for new Forms. On an edit they
        // may collide after a removal, so allocate every unmatched field a
        // fresh ID rather than allowing an additive field to masquerade as an
        // existing physical column.
        let mut next_id = used_ids.iter().copied().max().unwrap_or(99).max(99) + 1;
        for (name, field) in fields.iter_mut() {
            if stable_names.contains(name) {
                continue;
            }
            let Some(object) = field.as_object_mut() else {
                continue;
            };
            let candidate = object.get("id").and_then(Value::as_i64);
            let id = match candidate {
                Some(id) if id >= 100 && used_ids.insert(id) => id,
                _ => {
                    while used_ids.contains(&next_id) {
                        next_id += 1;
                    }
                    let id = next_id;
                    next_id += 1;
                    used_ids.insert(id);
                    id
                }
            };
            object.insert("id".to_string(), Value::from(id));
        }
    }
    Ok(())
}

pub(crate) fn enrich_form_definition(form_def: &Value) -> Result<Value> {
    let form_id = FormId::from(Uuid::parse_str(
        form_def
            .get("id")
            .and_then(Value::as_str)
            .context("Form definition missing stable 'id' field")?,
    )?);
    let name = form_def
        .get("name")
        .and_then(|v| v.as_str())
        .context("Form definition missing 'name' field")?;
    let template = form_template_from_fields(name, form_def.get("fields"));

    let mut enriched = form_def.clone();
    if let Some(obj) = enriched.as_object_mut() {
        obj.insert("template".to_string(), Value::String(template));
        obj.insert(
            "sql_relation".to_string(),
            Value::String(sql_relation_name(form_id)),
        );
        let fields = obj
            .get_mut("fields")
            .and_then(Value::as_object_mut)
            .context("Form definition missing 'fields' object")?;
        for field in fields.values_mut() {
            let field_id = FieldId::new(
                field
                    .get("id")
                    .and_then(Value::as_i64)
                    .context("Form field definition missing stable 'id' field")?
                    .try_into()
                    .context("Form field id is outside the supported range")?,
            )?;
            field
                .as_object_mut()
                .context("Form field definition must be an object")?
                .insert(
                    "sql_column".to_string(),
                    Value::String(sql_column_name(field_id)),
                );
        }
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
