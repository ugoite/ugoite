use crate::cli_config::SpaceTarget;
use crate::http;
use crate::output::{print_json_table, Format, UsageError};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_core::entry_query::{
    EntryFieldCapability, EntryFieldRef, EntryFilter, EntryPage, EntryPageRequest, EntryProjection,
    EntryQuery, EntryQueryScope, EntrySort, EntrySortDirection,
};
use ugoite_core::structured_search::SearchOperator;
use ugoite_domain::id::FormId;
use ugoite_iceberg::service::UgoiteService;

const CLI_ENTRY_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug)]
pub struct EntryListOptions {
    pub form: Option<String>,
    pub text: Option<String>,
    pub filters: Vec<String>,
    pub sorts: Vec<String>,
    pub columns: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
struct FormCapabilities {
    id: FormId,
    fields: BTreeMap<String, EntryFieldCapability>,
}

#[derive(Clone, Debug)]
struct ProjectionDisplay {
    projection: EntryProjection,
    columns: Vec<(String, String)>,
}

pub async fn list(target: &SpaceTarget, fmt: &Format, options: EntryListOptions) -> Result<()> {
    let target_rows = options.limit.unwrap_or(CLI_ENTRY_PAGE_SIZE);
    if target_rows == 0 {
        return Err(UsageError("--limit must be greater than zero".to_string()).into());
    }
    let space_id = target_space_id(target);
    let form = match options.form.as_deref() {
        Some(name) => Some(load_form(target, space_id, name).await?),
        None => None,
    };
    let scope = match form.as_ref() {
        Some(form) => EntryQueryScope::Form { form_id: form.id },
        None => EntryQueryScope::All,
    };
    let projection = build_projection(&scope, form.as_ref(), options.columns.as_deref())?;
    let query = EntryQuery {
        scope: scope.clone(),
        text: options
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        filters: options
            .filters
            .iter()
            .map(|raw| parse_filter(raw, &scope, form.as_ref()))
            .collect::<Result<Vec<_>>>()?,
        sort: options
            .sorts
            .iter()
            .map(|raw| parse_sort(raw, &scope, form.as_ref()))
            .collect::<Result<Vec<_>>>()?,
    };
    query
        .validate()
        .map_err(|error| UsageError(error.to_string()))?;

    let mut emitted = 0usize;
    let mut after = None;
    let mut json_first = true;
    if *fmt == Format::Json {
        print!("[");
    }
    loop {
        let request = EntryPageRequest {
            query: query.clone(),
            projection: projection.projection.clone(),
            limit: (target_rows - emitted).min(CLI_ENTRY_PAGE_SIZE),
            after: after.clone(),
        };
        let page = query_page(target, space_id, request).await?;
        if *fmt == Format::Json {
            for row in &page.rows {
                if !json_first {
                    print!(",");
                }
                print!("{}", serde_json::to_string(row)?);
                json_first = false;
            }
        } else if !page.rows.is_empty() {
            let display_rows = page
                .rows
                .iter()
                .map(|row| display_row(row, &projection))
                .collect::<Vec<_>>();
            let columns = projection
                .columns
                .iter()
                .map(|(header, key)| (header.as_str(), key.as_str()))
                .collect::<Vec<_>>();
            print_json_table(&display_rows, &columns);
        }
        emitted += page.rows.len();
        if emitted >= target_rows || !page.has_more {
            break;
        }
        after = Some(
            page.next
                .context("Entry query reported more rows without a continuation")?,
        );
    }
    if *fmt == Format::Json {
        println!("]");
    }
    Ok(())
}

fn target_space_id(target: &SpaceTarget) -> &str {
    match target {
        SpaceTarget::Core { space_id, .. }
        | SpaceTarget::Remote {
            space_uid: space_id,
            ..
        } => space_id,
    }
}

async fn load_form(target: &SpaceTarget, space_id: &str, name: &str) -> Result<FormCapabilities> {
    let value = match target {
        SpaceTarget::Remote { .. } => {
            http::execute_for_target(
                target,
                "form.get",
                serde_json::json!({"space_id": space_id, "form_name": name}),
                None,
            )
            .await?
        }
        SpaceTarget::Core { root, .. } => {
            UgoiteService::new_without_background_refresh(root)?
                .get_form(space_id, name)
                .await?
        }
    };
    parse_form_capabilities(&value, name)
}

fn parse_form_capabilities(value: &Value, name: &str) -> Result<FormCapabilities> {
    let id = serde_json::from_value::<FormId>(
        value
            .get("id")
            .cloned()
            .with_context(|| format!("Form {name} is missing its stable id"))?,
    )?;
    let fields = value
        .get("fields")
        .and_then(Value::as_object)
        .with_context(|| format!("Form {name} is missing its fields"))?
        .iter()
        .map(|(field_name, field)| {
            let capability = serde_json::from_value::<EntryFieldCapability>(
                field.get("query_capability").cloned().with_context(|| {
                    format!("Form field {field_name} is missing query capability")
                })?,
            )?;
            Ok((field_name.clone(), capability))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(FormCapabilities { id, fields })
}

fn build_projection(
    scope: &EntryQueryScope,
    form: Option<&FormCapabilities>,
    columns: Option<&str>,
) -> Result<ProjectionDisplay> {
    let Some(columns) = columns else {
        return Ok(ProjectionDisplay {
            projection: EntryProjection::Preview,
            columns: vec![("PREVIEW".to_string(), "preview".to_string())],
        });
    };
    let names = columns
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Err(UsageError("--columns must name at least one column".to_string()).into());
    }
    if names
        .iter()
        .any(|name| name.eq_ignore_ascii_case("preview"))
    {
        if names.len() != 1 {
            return Err(UsageError(
                "--columns preview cannot be combined with other columns".to_string(),
            )
            .into());
        }
        return Ok(ProjectionDisplay {
            projection: EntryProjection::Preview,
            columns: vec![("PREVIEW".to_string(), "preview".to_string())],
        });
    }
    let mut fields = Vec::with_capacity(names.len());
    let mut display = Vec::with_capacity(names.len());
    for name in names {
        let (field, key, header) = resolve_named_field(name, scope, form, "project")?;
        fields.push(field);
        display.push((header, key));
    }
    Ok(ProjectionDisplay {
        projection: EntryProjection::Fields { fields },
        columns: display,
    })
}

fn parse_filter(
    raw: &str,
    scope: &EntryQueryScope,
    form: Option<&FormCapabilities>,
) -> Result<EntryFilter> {
    let (field_and_operator, raw_value) = raw.split_once('=').ok_or_else(|| {
        UsageError(format!(
            "--filter must be FIELD[:OPERATOR]=VALUE, got {raw:?}"
        ))
    })?;
    let (field_name, operator_name) = field_and_operator
        .split_once(':')
        .map_or((field_and_operator, "equals"), |(field, operator)| {
            (field, operator)
        });
    let operator = parse_operator(operator_name)?;
    let (field, _, _) = resolve_named_field(field_name, scope, form, "filter")?;
    let capability = form.and_then(|form| form.fields.get(field_name));
    let value = parse_filter_value(raw_value, field, capability)?;
    if let Some(capability) = capability {
        if !capability.filterable || !capability.supported_operators.contains(&operator) {
            return Err(UsageError(format!(
                "operator {} is not supported for field {field_name:?}",
                operator.as_str()
            ))
            .into());
        }
    } else if field == EntryFieldRef::Form && operator != SearchOperator::Equals {
        return Err(UsageError("Form filters only support equals".to_string()).into());
    }
    Ok(EntryFilter {
        field,
        operator,
        value,
    })
}

fn parse_sort(
    raw: &str,
    scope: &EntryQueryScope,
    form: Option<&FormCapabilities>,
) -> Result<EntrySort> {
    let (field_name, direction_name) = raw
        .split_once(':')
        .map_or((raw, "asc"), |(field, direction)| (field, direction));
    let direction = match direction_name.trim().to_ascii_lowercase().as_str() {
        "asc" => EntrySortDirection::Asc,
        "desc" => EntrySortDirection::Desc,
        other => {
            return Err(
                UsageError(format!("sort direction must be asc or desc, got {other:?}")).into(),
            )
        }
    };
    let (field, _, _) = resolve_named_field(field_name, scope, form, "sort")?;
    if let Some(capability) = form.and_then(|form| form.fields.get(field_name)) {
        if !capability.sortable {
            return Err(UsageError(format!("field {field_name:?} is not sortable")).into());
        }
    }
    Ok(EntrySort { field, direction })
}

fn resolve_named_field(
    raw_name: &str,
    scope: &EntryQueryScope,
    form: Option<&FormCapabilities>,
    action: &str,
) -> Result<(EntryFieldRef, String, String)> {
    let name = raw_name.trim();
    if name.is_empty() {
        return Err(UsageError(format!("cannot {action} an empty field name")).into());
    }
    match name.to_ascii_lowercase().as_str() {
        "form" if matches!(scope, EntryQueryScope::All) => {
            return Ok((
                EntryFieldRef::Form,
                "form_id".to_string(),
                "FORM".to_string(),
            ))
        }
        "created" | "created_at" => {
            return Ok((
                EntryFieldRef::CreatedAt,
                "created_at_micros".to_string(),
                "CREATED".to_string(),
            ))
        }
        "updated" | "updated_at" => {
            return Ok((
                EntryFieldRef::UpdatedAt,
                "updated_at_micros".to_string(),
                "UPDATED".to_string(),
            ))
        }
        "form" => {
            return Err(UsageError("Form is only available in All Forms scope".to_string()).into())
        }
        _ => {}
    }
    let form = form.context("property fields require --form <FORM>")?;
    let capability = form
        .fields
        .get(name)
        .ok_or_else(|| UsageError(format!("unknown or non-capable Form field {name:?}")))?;
    if !capability.projectable && action == "project" {
        return Err(UsageError(format!("field {name:?} is not projectable")).into());
    }
    Ok((capability.field, name.to_string(), name.to_string()))
}

fn parse_operator(raw: &str) -> Result<SearchOperator> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "eq" | "equals" => Ok(SearchOperator::Equals),
        "contains" => Ok(SearchOperator::Contains),
        "lt" => Ok(SearchOperator::Lt),
        "lte" => Ok(SearchOperator::Lte),
        "gt" => Ok(SearchOperator::Gt),
        "gte" => Ok(SearchOperator::Gte),
        other => Err(UsageError(format!("unknown filter operator {other:?}")).into()),
    }
}

fn parse_filter_value(
    raw: &str,
    field: EntryFieldRef,
    capability: Option<&EntryFieldCapability>,
) -> Result<Value> {
    if field == EntryFieldRef::Form {
        return Ok(Value::String(raw.to_string()));
    }
    let Some(field_type) = capability.map(|capability| capability.field_type.as_str()) else {
        return Ok(Value::String(raw.to_string()));
    };
    match field_type {
        "boolean" => match raw.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(UsageError(format!("{raw:?} is not a boolean")).into()),
        },
        "integer" | "long" => Ok(Value::Number(
            raw.parse::<i64>()
                .map_err(|_| UsageError(format!("{raw:?} is not an integer")))?
                .into(),
        )),
        "float" | "double" => {
            let value = raw
                .parse::<f64>()
                .map_err(|_| UsageError(format!("{raw:?} is not a number")))?;
            let number = serde_json::Number::from_f64(value)
                .ok_or_else(|| UsageError(format!("{raw:?} is not a finite number")))?;
            Ok(Value::Number(number))
        }
        _ => Ok(Value::String(raw.to_string())),
    }
}

async fn query_page(
    target: &SpaceTarget,
    space_id: &str,
    request: EntryPageRequest,
) -> Result<EntryPage> {
    match target {
        SpaceTarget::Remote { .. } => {
            let value = http::execute_for_target(
                target,
                "entry.query",
                serde_json::json!({"space_id": space_id}),
                Some(serde_json::to_value(request)?),
            )
            .await?;
            Ok(serde_json::from_value(value)?)
        }
        SpaceTarget::Core { root, .. } => {
            UgoiteService::new_without_background_refresh(root)?
                .query_entry_page(space_id, request)
                .await
        }
    }
}

fn display_row(
    row: &ugoite_core::entry_query::EntryResult,
    projection: &ProjectionDisplay,
) -> Value {
    if projection.projection == EntryProjection::Preview {
        serde_json::json!({"preview": row.preview.clone().unwrap_or_default()})
    } else {
        let properties = row.properties.as_ref().and_then(Value::as_object);
        let mut display = serde_json::Map::new();
        for (_, key) in &projection.columns {
            let value = match key.as_str() {
                "created_at_micros" => Value::from(row.created_at_micros),
                "updated_at_micros" => Value::from(row.updated_at_micros),
                "form_id" => Value::String(row.form_id.to_string()),
                property => properties
                    .and_then(|values| values.get(property))
                    .cloned()
                    .unwrap_or(Value::Null),
            };
            display.insert(key.clone(), value);
        }
        Value::Object(display)
    }
}
