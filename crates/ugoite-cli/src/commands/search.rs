use crate::cli_config::resolve_command_triple;
use crate::http;
use crate::output::{
    effective_format, emit_success, print_json, print_json_table, Format, UsageError,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct SearchCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: SearchSubCmd,
}

#[derive(Subcommand)]
pub enum SearchSubCmd {
    /// Keyword search
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Keyword {
        #[arg(value_name = "QUERY")]
        query: String,
    },
    /// Typed structured search over Form fields
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Query(Box<SearchQueryArgs>),
}

/// Arguments for `search query`. Boxed at the enum site to keep the
/// subcommand enum size balanced.
#[derive(Args)]
pub struct SearchQueryArgs {
    #[arg(long, help = "Logical Form name to search.")]
    pub form: Option<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field equals condition. Repeatable."
    )]
    pub eq: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field contains condition. Repeatable."
    )]
    pub contains: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field less-than condition. Repeatable."
    )]
    pub lt: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field less-than-or-equal condition. Repeatable."
    )]
    pub lte: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field greater-than condition. Repeatable."
    )]
    pub gt: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD=VALUE",
        help = "Field greater-than-or-equal condition. Repeatable."
    )]
    pub gte: Vec<String>,
    #[arg(long, help = "Only entries updated at or after YYYY-MM-DD or RFC3339.")]
    pub updated_from: Option<String>,
    #[arg(long, help = "Only entries updated before YYYY-MM-DD or RFC3339.")]
    pub updated_to: Option<String>,
    #[arg(long, help = "Maximum rows to return.")]
    pub limit: Option<u64>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read criteria JSON from a file or stdin ('-'). Exclusive with condition flags."
    )]
    pub criteria_file: Option<String>,
}

/// Split one FIELD=VALUE flag payload. The CLI performs no type judgment;
/// values travel as strings for core normalization.
fn split_field_equals(raw: &str) -> Result<(String, String), UsageError> {
    let (field, value) = raw.split_once('=').ok_or_else(|| {
        UsageError(format!(
            "condition '{raw}' must have FIELD=VALUE shape (example: --eq status=open)"
        ))
    })?;
    if field.trim().is_empty() {
        return Err(UsageError(format!(
            "condition '{raw}' must name a non-empty field"
        )));
    }
    Ok((field.to_owned(), value.to_owned()))
}

fn flag_conditions(operator: &str, raws: &[String]) -> Result<Vec<serde_json::Value>, UsageError> {
    raws.iter()
        .map(|raw| {
            let (field, value) = split_field_equals(raw)?;
            Ok(serde_json::json!({
                "field": field,
                "operator": operator,
                "value": value,
            }))
        })
        .collect()
}

fn read_criteria_file(path: &str) -> Result<serde_json::Value> {
    let text = if path == "-" {
        use std::io::Read as _;
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read criteria from stdin")?;
        buffer
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read criteria file '{path}'"))?
    };
    serde_json::from_str(&text).with_context(|| format!("criteria file '{path}' must contain JSON"))
}

/// Condition flags for `search query`. Bundled so the criteria assembler
/// stays within the argument-count lint.
struct QueryFlags {
    form: Option<String>,
    eq: Vec<String>,
    contains: Vec<String>,
    lt: Vec<String>,
    lte: Vec<String>,
    gt: Vec<String>,
    gte: Vec<String>,
    updated_from: Option<String>,
    updated_to: Option<String>,
    limit: Option<u64>,
    criteria_file: Option<String>,
}

fn criteria_from_flags(flags: QueryFlags) -> Result<serde_json::Value, UsageError> {
    let QueryFlags {
        form,
        eq,
        contains,
        lt,
        lte,
        gt,
        gte,
        updated_from,
        updated_to,
        limit,
        criteria_file,
    } = flags;
    if let Some(path) = criteria_file.as_deref() {
        let mixed = form.is_some()
            || !eq.is_empty()
            || !contains.is_empty()
            || !lt.is_empty()
            || !lte.is_empty()
            || !gt.is_empty()
            || !gte.is_empty()
            || updated_from.is_some()
            || updated_to.is_some()
            || limit.is_some();
        if mixed {
            return Err(UsageError(
                "--criteria-file cannot be mixed with condition flags (--form, --eq, --contains, --lt, --lte, --gt, --gte, --updated-from, --updated-to, --limit)".to_string(),
            ));
        }
        let parsed = read_criteria_file(path).map_err(|error| UsageError(error.to_string()))?;
        // Accept the REST body shape {"criteria": {...}} as well as bare criteria.
        if let Some(inner) = parsed.get("criteria") {
            return Ok(inner.clone());
        }
        return Ok(parsed);
    }
    let Some(form) = form else {
        return Err(UsageError(
            "--form is required unless --criteria-file is used".to_string(),
        ));
    };
    let mut conditions = Vec::new();
    conditions.extend(flag_conditions("equals", &eq)?);
    conditions.extend(flag_conditions("contains", &contains)?);
    conditions.extend(flag_conditions("lt", &lt)?);
    conditions.extend(flag_conditions("lte", &lte)?);
    conditions.extend(flag_conditions("gt", &gt)?);
    conditions.extend(flag_conditions("gte", &gte)?);
    let mut criteria = serde_json::json!({
        "form": form,
        "conditions": conditions,
    });
    if let Some(value) = updated_from {
        criteria["updated_from"] = serde_json::Value::String(value);
    }
    if let Some(value) = updated_to {
        criteria["updated_to"] = serde_json::Value::String(value);
    }
    if let Some(value) = limit {
        criteria["limit"] = serde_json::json!(value);
    }
    Ok(criteria)
}

/// Concise TTY projection for structured rows. Piped output keeps full JSON.
fn criteria_rows_table(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let id = row
                .get("_ugoite_id")
                .or_else(|| row.get("id"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            serde_json::json!({"id": id})
        })
        .collect()
}

pub async fn run(
    cmd: SearchCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SearchSubCmd::Keyword { query } => {
            let (root, space_id, base) =
                resolve_command_triple(explicit_config, context_override, "search keyword")?;
            if base.is_some() {
                let target = crate::cli_config::resolve_command_target(
                    explicit_config,
                    context_override,
                    "search keyword",
                )?;
                let result = http::execute_for_target(
                    &target,
                    "search.keyword",
                    serde_json::json!({"space_id": space_id, "q": query}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(rows) = result.as_array() {
                        let table = criteria_rows_table(rows);
                        print_json_table(&table, &[("ID", "id")]);
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let results = service.search_entries(&space_id, &query).await?;
            if fmt != Format::Json {
                let rows: Vec<serde_json::Value> = results
                    .iter()
                    .map(|result| serde_json::to_value(result).expect("keyword result is JSON"))
                    .collect();
                let table = criteria_rows_table(&rows);
                print_json_table(&table, &[("ID", "id")]);
            } else {
                print_json(&results);
            }
        }
        SearchSubCmd::Query(args) => {
            let SearchQueryArgs {
                form,
                eq,
                contains,
                lt,
                lte,
                gt,
                gte,
                updated_from,
                updated_to,
                limit,
                criteria_file,
            } = *args;
            let criteria_value = criteria_from_flags(QueryFlags {
                form,
                eq,
                contains,
                lt,
                lte,
                gt,
                gte,
                updated_from,
                updated_to,
                limit,
                criteria_file,
            })
            .map_err(anyhow::Error::from)?;
            let (root, space_id, base) =
                resolve_command_triple(explicit_config, context_override, "search query")?;
            if base.is_some() {
                let target = crate::cli_config::resolve_command_target(
                    explicit_config,
                    context_override,
                    "search query",
                )?;
                let result = http::execute_for_target(
                    &target,
                    "search.query",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"criteria": criteria_value})),
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(rows) = result.as_array() {
                        let table = criteria_rows_table(rows);
                        print_json_table(&table, &[("ID", "id")]);
                        return Ok(());
                    }
                }
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let criteria: ugoite_core::structured_search::StructuredSearch =
                serde_json::from_value(criteria_value).map_err(|error| {
                    UsageError(format!("invalid structured search criteria: {error}"))
                })?;
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let rows = service.search_structured(&space_id, &criteria).await?;
            if fmt != Format::Json {
                let table = criteria_rows_table(&rows);
                print_json_table(&table, &[("ID", "id")]);
            } else {
                emit_success(&rows, &fmt, None);
            }
        }
    }
    Ok(())
}
