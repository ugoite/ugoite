use crate::cli_config::{
    resolve_command_target, split_space_and_id, split_space_id_and_revision, SpaceTarget,
};
use crate::http;
use crate::output::{
    effective_format, emit_success, print_json_table, read_compat_input, render_receipt,
    stdout_style, Format, MutationReceipt, UsageError,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct EntryCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: EntrySubCmd,
}

#[derive(Subcommand)]
pub enum EntrySubCmd {
    /// List entries in a space
    #[command(
        long_about = "List entries in a space.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry list\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry list\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry list /root/spaces/my-space\n  ugoite entry list 019f1234-5678-7abc-8def-0123456789ab"
    )]
    List {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
    },
    /// Get an entry by ID
    #[command(
        long_about = "Get an entry by ID.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry get my-entry-id\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry get my-entry-id\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry get /root/spaces/my-space my-entry-id\n  ugoite entry get 019f1234-5678-7abc-8def-0123456789ab my-entry-id"
    )]
    Get {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID",
            num_args(1..=2),
            required = true,
            help = "ENTRY_ID against the selected context (Entry slug/ID, e.g. 'my-note', 'task-01'), or legacy SPACE ENTRY_ID."
        )]
        space_and_id: Vec<String>,
    },
    /// Create an entry
    #[command(
        long_about = "Create an entry in a space.\n\nThe entry ID is a slug (alphanumeric + hyphens). Structured authoring is recommended; raw Markdown is the 0.1.x compatibility surface.\n\nExamples (structured, preferred):\n  # Selected context (no Space argument)\n  ugoite entry create task-01 --form Task --field status=open --field priority=3\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry create task-01 --form Task --fields-file fields.json\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry create /root/spaces/my-space task-01 --form Task --field status=open\n  ugoite entry create 019f1234-5678-7abc-8def-0123456789ab task-01 --form Task --fields-file fields.json\n\n  # 0.1.x compatibility only: --title preserves a legacy display title and will be removed in 0.2\n  ugoite entry create /root/spaces/my-space task-01 --form Task --title 'Ship 0.1.x' --field status=open\n\nExamples (raw Markdown, 0.1.x compatibility):\n  # Selected context (no Space argument)\n  ugoite entry create my-note --content '# My Note'\n\n  # Selected context override for one invocation\n  ugoite --context NAME entry create my-note --file ./note.md\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry create /root/spaces/my-space my-note --content '# My Note'\n  cat ./note.md | ugoite entry create /root/spaces/my-space my-note --file -\n\nFrontmatter is optional in the compatibility path and only needed when you want form-backed metadata.\n\nAsset attachment uses this structured path: first `asset upload` the bytes, then put the returned asset object as the field value in --fields-file JSON (for example {\"Document\": {\"asset_id\": \"...\", \"name\": \"report.txt\", \"media_type\": \"...\", \"size_bytes\": 36, \"sha256\": \"...\"}}). There is no dedicated attach flag; --field KEY=VALUE stays a string and cannot carry an asset object."
    )]
    Create {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID",
            num_args(1..=2),
            required = true,
            help = "ENTRY_ID against the selected context (Entry slug/ID, e.g. 'my-note', 'task-01'), or legacy SPACE ENTRY_ID."
        )]
        space_and_id: Vec<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Entry content as a Markdown string (supports frontmatter for form/tags)"
        )]
        content: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read Markdown content from PATH, or from explicit stdin with --file - (cannot combine with --content)"
        )]
        file: Option<String>,
        #[arg(
            long,
            value_name = "FORM",
            help = "Form name for structured authoring (requires no --content/--file)"
        )]
        form: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Legacy 0.1.x compatibility title for structured authoring (omit for title-less entries; removed in 0.2)"
        )]
        title: Option<String>,
        #[arg(
            long = "field",
            value_name = "KEY=VALUE",
            help = "Structured field as KEY=VALUE (repeatable; VALUE stays a string and the shared Rust boundary coerces it)"
        )]
        fields: Vec<String>,
        #[arg(
            long = "fields-file",
            value_name = "PATH",
            help = "Read structured fields as a JSON object from PATH, or from explicit stdin with --fields-file - (duplicate inputs are errors; duplicate keys within one JSON object use the parser's last value)"
        )]
        fields_files: Vec<String>,
        #[arg(
            long,
            help = "Author name to record in the revision history (local only)"
        )]
        author: Option<String>,
    },
    /// Update an entry
    #[command(
        long_about = "Update an entry in a space.\n\nStructured authoring is recommended; raw Markdown is the 0.1.x compatibility surface. --field/--fields-file values are the complete post-update field map: omitted fields are cleared, never patched. Tags and extra attributes are preserved because they are not editable through this CLI command; an explicit --field/--fields-file value replaces a preserved extra attribute with the same key.\n\nExamples (structured, preferred):\n  # Selected context (no Space argument)\n  ugoite entry update task-01 --fields-file entry-fields.json --parent-revision-id rev-1\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry update task-01 --field status=done --parent-revision-id rev-1\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry update /root/spaces/my-space task-01 --fields-file entry-fields.json --parent-revision-id rev-1\n  ugoite entry update 019f1234-5678-7abc-8def-0123456789ab task-01 --title 'New title' --fields-file entry-fields.json --parent-revision-id rev-1\n\nExamples (raw Markdown, 0.1.x compatibility):\n  # Selected context (no Space argument)\n  ugoite entry update my-note --markdown '# Updated'\n\n  # Selected context override for one invocation\n  ugoite --context NAME entry update my-note --markdown '# Updated' --parent-revision-id rev-1\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry update /root/spaces/my-space my-note --markdown '# Updated'\n  ugoite entry update 019f1234-5678-7abc-8def-0123456789ab my-note --markdown '# Updated'\n\nWhen --parent-revision-id is omitted, the CLI reads the current Entry immediately before the update and uses its revision ID for optimistic concurrency.\n\nStructured updates carry the complete post-update field map: to keep an existing attachment while adding another, read the current entry first and resupply the full Attachments array in --fields-file. Omitted list items are dropped, never merged."
    )]
    Update {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID",
            num_args(1..=2),
            required = true,
            help = "ENTRY_ID against the selected context (Entry slug/ID, e.g. 'my-note', 'task-01'), or legacy SPACE ENTRY_ID."
        )]
        space_and_id: Vec<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Updated entry content as a Markdown string (must keep the same form frontmatter)"
        )]
        markdown: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read Markdown content from PATH, or from explicit stdin with --file - (cannot combine with --markdown)"
        )]
        file: Option<String>,
        #[arg(
            long,
            value_name = "FORM",
            help = "Form name for structured authoring (must match the stored form; changes are rejected)"
        )]
        form: Option<String>,
        #[arg(
            long,
            allow_hyphen_values = true,
            help = "Legacy 0.1.x compatibility title for structured authoring (omit to leave titles untouched; removed in 0.2)"
        )]
        title: Option<String>,
        #[arg(
            long = "field",
            value_name = "KEY=VALUE",
            help = "Structured field as KEY=VALUE (repeatable; VALUE stays a string and the shared Rust boundary coerces it)"
        )]
        fields: Vec<String>,
        #[arg(
            long = "fields-file",
            value_name = "PATH",
            help = "Read structured fields as a JSON object from PATH, or from explicit stdin with --fields-file - (duplicate inputs are errors; duplicate keys within one JSON object use the parser's last value)"
        )]
        fields_files: Vec<String>,
        #[arg(
            long,
            help = "Expected current revision ID to enforce optimistic concurrency checks; if omitted, the CLI reads it immediately before updating"
        )]
        parent_revision_id: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (local only)"
        )]
        author: String,
    },
    /// Delete an entry
    #[command(
        long_about = "Delete an entry from a space.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry delete my-note\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry delete my-note\n\n  # 0.1.x compatibility only: legacy explicit Space (remote deletions require a human approval token)\n  ugoite entry delete /root/spaces/my-space my-note\n  ugoite entry delete 019f1234-5678-7abc-8def-0123456789ab my-note --human-approval <token>"
    )]
    Delete {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID",
            num_args(1..=2),
            required = true,
            help = "ENTRY_ID against the selected context (Entry slug/ID, e.g. 'my-note', 'task-01'), or legacy SPACE ENTRY_ID."
        )]
        space_and_id: Vec<String>,
        #[arg(long)]
        hard_delete: bool,
        /// Single-use approval token issued by a recently reauthenticated human.
        #[arg(long)]
        human_approval: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Actor name to record for the delete (local only)"
        )]
        author: String,
    },
    /// Get entry history
    #[command(
        long_about = "Get the revision history of an entry.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry history my-note\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry history my-note\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry history /root/spaces/my-space my-note\n  ugoite entry history 019f1234-5678-7abc-8def-0123456789ab my-note"
    )]
    History {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID",
            num_args(1..=2),
            required = true,
            help = "ENTRY_ID against the selected context (Entry slug/ID, e.g. 'my-note', 'task-01'), or legacy SPACE ENTRY_ID."
        )]
        space_and_id: Vec<String>,
    },
    /// Get a specific revision
    #[command(
        long_about = "Get a specific revision of an entry.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry revision my-note rev-1\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry revision my-note rev-1\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry revision /root/spaces/my-space my-note rev-1\n  ugoite entry revision 019f1234-5678-7abc-8def-0123456789ab my-note rev-1"
    )]
    Revision {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID_AND_REVISION",
            num_args(2..=3),
            required = true,
            help = "ENTRY_ID REVISION_ID against the selected context, or legacy SPACE ENTRY_ID REVISION_ID."
        )]
        space_id_and_revision: Vec<String>,
    },
    /// Restore an entry to a revision
    #[command(
        long_about = "Restore an entry to a previous revision.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite entry restore my-note rev-1\n\n  # Selected context override for one invocation (does not change the selection)\n  ugoite --context NAME entry restore my-note rev-1\n\n  # 0.1.x compatibility only: legacy explicit Space\n  ugoite entry restore /root/spaces/my-space my-note rev-1\n  ugoite entry restore 019f1234-5678-7abc-8def-0123456789ab my-note rev-1"
    )]
    Restore {
        #[arg(
            value_name = "SPACE_OR_ENTRY_ID_AND_REVISION",
            num_args(2..=3),
            required = true,
            help = "ENTRY_ID REVISION_ID against the selected context, or legacy SPACE ENTRY_ID REVISION_ID."
        )]
        space_id_and_revision: Vec<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record in the revision history (local only)"
        )]
        author: String,
    },
}

fn entry_receipt(
    id: String,
    revision_id: Option<String>,
    change_id: Option<String>,
) -> MutationReceipt {
    MutationReceipt::entry(id, revision_id, change_id)
}

fn current_entry_revision_id(entry: &serde_json::Value) -> Result<String> {
    entry
        .get("revision_id")
        .and_then(serde_json::Value::as_str)
        .filter(|revision_id| !revision_id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("current entry response is missing revision_id"))
}

async fn read_remote_entry_revision_id(
    target: &SpaceTarget,
    space_id: &str,
    entry_id: &str,
) -> Result<String> {
    let entry = http::execute_for_target(
        target,
        "entry.get",
        serde_json::json!({"space_id": space_id, "entry_id": entry_id}),
        None,
    )
    .await?;
    current_entry_revision_id(&entry)
}

fn entry_object_map(
    entry: &serde_json::Value,
    key: &str,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>> {
    let value = entry
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("current entry response is missing {key}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("current entry response field {key} must be an object"))?;
    Ok(object.clone().into_iter().collect())
}

fn validate_entry_input_style(
    has_structured: bool,
    has_markdown: bool,
    markdown_flags: &str,
) -> Result<()> {
    if has_structured && has_markdown {
        return Err(UsageError(format!(
            "structured options (--form/--title/--field/--fields-file) and {markdown_flags} cannot be combined; specify exactly one input style"
        ))
        .into());
    }
    Ok(())
}

/// Parse one `--field KEY=VALUE` argument. Values stay strings; the shared
/// Rust boundary owns all coercion, so the CLI never interprets types here.
fn parse_field_arg(arg: &str) -> Result<(String, serde_json::Value), UsageError> {
    let (key, value) = arg.split_once('=').ok_or_else(|| {
        UsageError(format!(
            "--field must be KEY=VALUE, got {arg:?}; complex values belong in --fields-file JSON"
        ))
    })?;
    if key.is_empty() {
        return Err(UsageError(format!(
            "--field must be KEY=VALUE with a non-empty key, got {arg:?}"
        )));
    }
    Ok((
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    ))
}

/// Read one `--fields-file` JSON object (or `-` for explicit stdin). Values
/// stay typed JSON; the shared Rust boundary owns coercion and validation.
fn read_fields_file(
    path: &str,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, anyhow::Error> {
    let text = if path == "-" {
        use std::io::Read;
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| anyhow::anyhow!("read stdin: {error}"))?;
        text
    } else {
        std::fs::read_to_string(path)
            .map_err(|error| UsageError(format!("read --fields-file {path}: {error}")))?
    };
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| UsageError(format!("parse --fields-file {path}: {error}")))?;
    match parsed {
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(UsageError(format!(
            "--fields-file {path} must contain a JSON object mapping field names to values"
        ))
        .into()),
    }
}

/// Merge `--field` and `--fields-file` inputs. Duplicate keys are a
/// deterministic usage error so concurrent authors never silently win.
fn merge_structured_fields(
    fields: Vec<String>,
    fields_files: Vec<String>,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, anyhow::Error> {
    let mut merged = std::collections::BTreeMap::new();
    let mut duplicates = std::collections::BTreeSet::new();
    for path in &fields_files {
        for (key, value) in read_fields_file(path)? {
            if merged.insert(key.clone(), value).is_some() {
                duplicates.insert(key);
            }
        }
    }
    for arg in &fields {
        let (key, value) = parse_field_arg(arg).map_err(anyhow::Error::from)?;
        if merged.insert(key.clone(), value).is_some() {
            duplicates.insert(key);
        }
    }
    if !duplicates.is_empty() {
        let names: Vec<String> = duplicates.into_iter().collect();
        let noun = if names.len() == 1 { "field" } else { "fields" };
        return Err(UsageError(format!(
            "duplicate {noun} {} from --field/--fields-file; specify each field once",
            names.join(", ")
        ))
        .into());
    }
    Ok(merged)
}

#[allow(clippy::too_many_arguments)]
async fn create_structured_entry(
    target: &SpaceTarget,
    fmt: &Format,
    entry_id: String,
    form: Option<String>,
    title: Option<String>,
    fields: Vec<String>,
    fields_files: Vec<String>,
    author: Option<String>,
) -> Result<()> {
    let Some(form_name) = form else {
        return Err(
            UsageError("--form is required for structured entry create".to_string()).into(),
        );
    };
    let merged = merge_structured_fields(fields, fields_files)?;
    if let SpaceTarget::Remote { space_uid, .. } = target {
        if author.is_some() {
            return Err(UsageError(
                "entry create --author is only supported in core mode; backend/api derive author from the authenticated identity"
                    .to_string(),
            )
            .into());
        }
        let mut body = serde_json::json!({
            "id": entry_id,
            "form": form_name,
            "fields": merged,
        });
        if let Some(title) = title.as_deref() {
            body["title"] = serde_json::json!(title);
        }
        let result = http::execute_for_target(
            target,
            "entry.create",
            serde_json::json!({"space_id": space_uid}),
            Some(body),
        )
        .await?;
        let receipt = entry_receipt(
            entry_id,
            result
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            result
                .get("change_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        );
        emit_success(
            &result,
            fmt,
            Some(render_receipt(&receipt, &stdout_style())),
        );
        return Ok(());
    }
    let SpaceTarget::Core { root, space_id } = target else {
        anyhow::bail!("operation entry.create does not use the remote transport")
    };
    let author = author.unwrap_or_else(|| "cli".to_string());
    let service = UgoiteService::new_without_background_refresh(root)?;
    let (meta, _commit_receipt) = service
        .create_structured_entry_with_receipt(
            space_id,
            &entry_id,
            title,
            form_name,
            Vec::new(),
            merged,
            std::collections::BTreeMap::new(),
            &author,
        )
        .await?;
    let receipt = entry_receipt(
        entry_id,
        meta.get("revision_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        meta.get("change_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    emit_success(&meta, fmt, Some(render_receipt(&receipt, &stdout_style())));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_structured_entry(
    target: &SpaceTarget,
    fmt: &Format,
    entry_id: String,
    form: Option<String>,
    title: Option<String>,
    fields: Vec<String>,
    fields_files: Vec<String>,
    parent_revision_id: Option<String>,
    author: String,
) -> Result<()> {
    if fields.is_empty() && fields_files.is_empty() {
        return Err(UsageError(
            "structured update requires --field or --fields-file carrying the complete post-update field map"
                .to_string(),
        )
        .into());
    }
    let fields = merge_structured_fields(fields, fields_files)?;
    if let SpaceTarget::Remote { space_uid, .. } = target {
        if author != "cli" {
            return Err(UsageError(
                "entry update --author is only supported in core mode; backend/api derive author from the authenticated identity"
                    .to_string(),
            )
            .into());
        }
        let current = http::execute_for_target(
            target,
            "entry.get",
            serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
            None,
        )
        .await?;
        let mut extra_attributes = entry_object_map(&current, "extra_attributes")?;
        // Explicit structured inputs are the complete post-update field map:
        // they replace preserved extra_attributes on key overlap. The shared
        // Rust boundary rejects overlap instead of preferring a side, so the
        // caller resolves it here, explicitly, before sending.
        for key in fields.keys() {
            extra_attributes.remove(key);
        }
        let parent_revision_id = match parent_revision_id {
            Some(parent_revision_id) => parent_revision_id,
            None => current_entry_revision_id(&current)?,
        };
        let mut body = serde_json::json!({
            "fields": fields,
            "extra_attributes": extra_attributes,
        });
        if let Some(form) = form.as_deref() {
            body["form"] = serde_json::json!(form);
        }
        if let Some(title) = title.as_deref() {
            body["title"] = serde_json::json!(title);
        }
        body["parent_revision_id"] = serde_json::json!(parent_revision_id);
        let result = http::execute_for_target(
            target,
            "entry.update",
            serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
            Some(body),
        )
        .await?;
        let receipt = entry_receipt(
            entry_id,
            result
                .get("revision_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            result
                .get("change_id")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        );
        emit_success(
            &result,
            fmt,
            Some(render_receipt(&receipt, &stdout_style())),
        );
        return Ok(());
    }
    let SpaceTarget::Core { root, space_id } = target else {
        anyhow::bail!("operation entry.update does not use the remote transport")
    };
    let service = UgoiteService::new_without_background_refresh(root)?;
    let current = service.get_entry(space_id, &entry_id).await?;
    let mut extra_attributes = entry_object_map(&current, "extra_attributes")?;
    // Explicit structured inputs are the complete post-update field map:
    // they replace preserved extra_attributes on key overlap. The shared
    // Rust boundary rejects overlap instead of preferring a side, so the
    // caller resolves it here, explicitly, before sending.
    for key in fields.keys() {
        extra_attributes.remove(key);
    }
    let parent_revision_id = match parent_revision_id {
        Some(parent_revision_id) => parent_revision_id,
        None => current_entry_revision_id(&current)?,
    };
    let result = service
        .update_structured_entry(
            space_id,
            &entry_id,
            title,
            form,
            fields,
            extra_attributes,
            Some(&parent_revision_id),
            &author,
        )
        .await?;
    let receipt = entry_receipt(
        entry_id,
        result
            .get("revision_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        result
            .get("change_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    emit_success(
        &result,
        fmt,
        Some(render_receipt(&receipt, &stdout_style())),
    );
    Ok(())
}

pub async fn run(
    cmd: EntryCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        EntrySubCmd::List { space_path } => {
            let target = resolve_command_target(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "entry list",
            )?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.list",
                    serde_json::json!({"space_id": space_uid}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id"), ("TITLE", "title")]);
                        return Ok(());
                    }
                }
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let entries = service.list_entries(space_id).await?;
            if fmt != Format::Json {
                let rows: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "title": e.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        })
                    })
                    .collect();
                print_json_table(&rows, &[("ID", "id"), ("TITLE", "title")]);
            } else {
                emit_success(&entries, &fmt, None);
            }
        }
        EntrySubCmd::Get { space_and_id } => {
            let (legacy_space, entry_id) =
                split_space_and_id(&space_and_id, "ENTRY_ID", "entry get")?;
            let entry_id = entry_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry get",
            )?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.get",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.get does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let entry = service.get_entry(space_id, &entry_id).await?;
            emit_success(&entry, &fmt, None);
        }
        EntrySubCmd::Create {
            space_and_id,
            content,
            file,
            form,
            title,
            fields,
            fields_files,
            author,
        } => {
            let (legacy_space, entry_id) =
                split_space_and_id(&space_and_id, "ENTRY_ID", "entry create")?;
            let entry_id = entry_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry create",
            )?;
            let has_structured =
                form.is_some() || title.is_some() || !fields.is_empty() || !fields_files.is_empty();
            let has_markdown = content.is_some() || file.is_some();
            validate_entry_input_style(has_structured, has_markdown, "--content/--file")?;
            if has_structured {
                if title.is_some() {
                    eprintln!(
                        "note: --title is a 0.1.x legacy compatibility option; omit it for title-less entries (removed in 0.2)"
                    );
                }
                return create_structured_entry(
                    &target,
                    &fmt,
                    entry_id,
                    form,
                    title,
                    fields,
                    fields_files,
                    author,
                )
                .await;
            }
            // Shell-safe compatibility ingress: inline and file are mutually
            // exclusive; neither provided falls back to the default note.
            let content = match (content, file) {
                (Some(_), Some(_)) => {
                    return Err(UsageError(
                        "--content and --file cannot be combined; specify exactly one".to_string(),
                    )
                    .into());
                }
                (Some(text), None) => text,
                (None, Some(path)) => read_compat_input(None, "--content", Some(path))?,
                (None, None) => "# New Entry\n".to_string(),
            };
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author.is_some() {
                    return Err(UsageError(
                        "entry create --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "entry.create",
                    serde_json::json!({"space_id": space_uid}),
                    Some(serde_json::json!({"id": entry_id, "markdown": content})),
                )
                .await?;
                // 0.1.x machine contract: keep the existing output shape.
                // The receipt is TTY display only; switching the machine
                // default to the receipt is a v0.2 interface decision.
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.create does not use the remote transport")
            };
            let author = author.unwrap_or_else(|| "cli".to_string());
            // A mutation schedules the process-local coalesced refresh but
            // never drains it; the authoritative commit is the CLI latency
            // boundary and `ugoite index run` is the explicit repair command.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let (mut meta, commit_receipt) = service
                .create_entry_with_receipt(space_id, &entry_id, &content, &author)
                .await?;
            meta["change_id"] = serde_json::json!(commit_receipt.command_id);
            let receipt = entry_receipt(
                entry_id,
                meta.get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                meta.get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(&meta, &fmt, Some(render_receipt(&receipt, &stdout_style())));
        }
        EntrySubCmd::Update {
            space_and_id,
            markdown,
            file,
            form,
            title,
            fields,
            fields_files,
            parent_revision_id,
            author,
        } => {
            let (legacy_space, entry_id) =
                split_space_and_id(&space_and_id, "ENTRY_ID", "entry update")?;
            let entry_id = entry_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry update",
            )?;
            let has_structured =
                form.is_some() || title.is_some() || !fields.is_empty() || !fields_files.is_empty();
            let has_markdown = markdown.is_some() || file.is_some();
            validate_entry_input_style(has_structured, has_markdown, "--markdown/--file")?;
            if has_structured {
                if title.is_some() {
                    eprintln!(
                        "note: --title is a 0.1.x legacy compatibility option; omit it to leave titles untouched (removed in 0.2)"
                    );
                }
                return update_structured_entry(
                    &target,
                    &fmt,
                    entry_id,
                    form,
                    title,
                    fields,
                    fields_files,
                    parent_revision_id,
                    author,
                )
                .await;
            }
            let markdown = read_compat_input(markdown, "--markdown", file)?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "entry update --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let parent_revision_id = match parent_revision_id {
                    Some(parent_revision_id) => parent_revision_id,
                    None => read_remote_entry_revision_id(&target, space_uid, &entry_id).await?,
                };
                let mut body = serde_json::json!({"markdown": markdown});
                body["parent_revision_id"] = serde_json::json!(parent_revision_id);
                let result = http::execute_for_target(
                    &target,
                    "entry.update",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    Some(body),
                )
                .await?;
                // 0.1.x machine contract: keep the existing output shape (see create).
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.update does not use the remote transport")
            };
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let parent_revision_id = match parent_revision_id {
                Some(parent_revision_id) => parent_revision_id,
                None => current_entry_revision_id(&service.get_entry(space_id, &entry_id).await?)?,
            };
            let result = service
                .update_entry(
                    space_id,
                    &entry_id,
                    &markdown,
                    Some(&parent_revision_id),
                    &author,
                )
                .await?;
            let receipt = entry_receipt(
                entry_id,
                result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(
                &result,
                &fmt,
                Some(render_receipt(&receipt, &stdout_style())),
            );
        }
        EntrySubCmd::Delete {
            space_and_id,
            hard_delete,
            human_approval,
            author,
        } => {
            let (legacy_space, entry_id) =
                split_space_and_id(&space_and_id, "ENTRY_ID", "entry delete")?;
            let entry_id = entry_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry delete",
            )?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "entry delete --author is only supported in core mode; backend/api derive actor from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "entry.delete",
                    serde_json::json!({
                        "space_id": space_uid,
                        "entry_id": entry_id,
                        "hard_delete": hard_delete,
                        "human_approval": human_approval,
                    }),
                    None,
                )
                .await?;
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            if human_approval.is_some() {
                return Err(UsageError(
                    "--human-approval is only supported in backend/api mode".to_string(),
                )
                .into());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.delete does not use the remote transport")
            };
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .delete_entry_with_receipt(space_id, &entry_id, hard_delete, &author)
                .await?;
            let receipt = entry_receipt(
                entry_id,
                result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(
                &result,
                &fmt,
                Some(render_receipt(&receipt, &stdout_style())),
            );
        }
        EntrySubCmd::History { space_and_id } => {
            let (legacy_space, entry_id) =
                split_space_and_id(&space_and_id, "ENTRY_ID", "entry history")?;
            let entry_id = entry_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry history",
            )?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.history",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.history does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let history = service.entry_history(space_id, &entry_id).await?;
            emit_success(&history, &fmt, None);
        }
        EntrySubCmd::Revision {
            space_id_and_revision,
        } => {
            let (legacy_space, entry_id, revision_id) =
                split_space_id_and_revision(&space_id_and_revision, "entry revision")?;
            let entry_id = entry_id.to_string();
            let revision_id = revision_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry revision",
            )?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "entry.revision",
                    serde_json::json!({
                        "space_id": space_uid,
                        "entry_id": entry_id,
                        "revision_id": revision_id,
                    }),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.revision does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let rev = service
                .entry_revision(space_id, &entry_id, &revision_id)
                .await?;
            emit_success(&rev, &fmt, None);
        }
        EntrySubCmd::Restore {
            space_id_and_revision,
            author,
        } => {
            let (legacy_space, entry_id, revision_id) =
                split_space_id_and_revision(&space_id_and_revision, "entry restore")?;
            let entry_id = entry_id.to_string();
            let revision_id = revision_id.to_string();
            let target = resolve_command_target(
                legacy_space,
                explicit_config,
                context_override,
                "entry restore",
            )?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "entry restore --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "entry.restore",
                    serde_json::json!({"space_id": space_uid, "entry_id": entry_id}),
                    Some(serde_json::json!({"revision_id": revision_id})),
                )
                .await?;
                let receipt = entry_receipt(
                    entry_id,
                    result
                        .get("revision_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    result
                        .get("change_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                );
                emit_success(
                    &result,
                    &fmt,
                    Some(render_receipt(&receipt, &stdout_style())),
                );
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation entry.restore does not use the remote transport")
            };
            // Do not wait for Derived refreshes in a one-shot mutation.
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .restore_entry(space_id, &entry_id, &revision_id, &author)
                .await?;
            let receipt = entry_receipt(
                entry_id,
                result
                    .get("revision_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            );
            emit_success(
                &result,
                &fmt,
                Some(render_receipt(&receipt, &stdout_style())),
            );
        }
    }
    Ok(())
}
