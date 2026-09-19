use crate::config::{
    effective_format, load_config, normalize_space_root, operator_for_path, parse_space_path,
    print_json, print_json_table, print_list_table, resolve_backend_space_uid,
    resolve_space_reference, validated_base_url, EndpointConfig, Format,
};
use crate::http;
use crate::step_up;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::Path;
use ugoite_iceberg::sample_data::SampleDataOptions;
use ugoite_iceberg::service::{validate_public_space_patch, UgoiteService};

fn backend_api_mode_error(config: &EndpointConfig, command_name: &str) -> String {
    format!(
        "{command_name} requires backend or api mode.\nRun `ugoite config current` to inspect the active mode, then switch with `ugoite config set --mode backend --backend-url {}` or `ugoite config set --mode api --api-url {}`.",
        config.backend_url, config.api_url
    )
}

#[derive(Args)]
pub struct SpaceCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: SpaceSubCmd,
}

#[derive(Subcommand)]
pub enum SpaceSubCmd {
    /// Create a new space
    #[command(
        long_about = "Create a new space.\n\nRun `ugoite config current` to check whether you are in core, backend, or api mode. The positional value is a local Space path in core mode or the new human-readable Space slug in backend/api mode. A server-generated Space UID is returned after creation and is the authority for all later operations; the requested slug is never a UID.\n\nExamples:\n  # Core mode (full local Space path, optional display name)\n  ugoite space create /root/spaces/my-space --name \"My Space\"\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite space create team-notes --name \"Team Notes\""
    )]
    Create {
        #[arg(
            value_name = "SPACE_SLUG_OR_PATH",
            help = "New Space slug in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(
            long,
            value_name = "DISPLAY_NAME",
            help = "Display name for the new Space; defaults to the requested slug."
        )]
        name: Option<String>,
        #[arg(
            long,
            value_name = "CONNECTION",
            help = "Canonical connection to create the Space on. Defaults to the current context's connection, or the only defined connection."
        )]
        connection: Option<String>,
        #[arg(
            long,
            help = "Create the Space without registering a CLI context (no config changes)."
        )]
        no_context: bool,
    },
    /// List spaces
    #[command(
        long_about = "List all spaces.\n\nRun `ugoite config current` to check whether you should pass a local `ROOT_PATH` or omit it entirely.\nUse `ROOT_PATH` in core mode and omit it in backend/api mode.\n\nExamples:\n  # Core mode (workspace root)\n  ugoite space list /root\n\n  # Core mode (spaces directory also accepted)\n  ugoite space list /root/spaces\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite space list"
    )]
    List {
        #[arg(
            value_name = "ROOT_PATH",
            help = "Workspace root in core mode (for example /root or /root/spaces). Omit in backend/api mode."
        )]
        root_path: Option<String>,
    },
    /// Get space metadata
    #[command(
        long_about = "Get space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or an immutable `SPACE_UID`.\n\nExamples:\n  # Core mode\n  ugoite space get /root/spaces/my-space\n\n  # Backend mode (immutable Space UID)\n  ugoite space get 019f1234-5678-7abc-8def-0123456789ab"
    )]
    Get {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
    },
    /// Patch space metadata
    #[command(
        long_about = "Patch space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or an immutable `SPACE_UID`.\n\nExamples:\n  # Core mode\n  ugoite space patch /root/spaces/my-space --name \"Renamed Space\"\n\n  # Backend mode (immutable Space UID)\n  ugoite space patch 019f1234-5678-7abc-8def-0123456789ab --settings '{\"theme\":\"dark\"}'"
    )]
    Patch {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        storage_config: Option<String>,
        #[arg(long)]
        settings: Option<String>,
    },
    /// Create sample data
    SampleData {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_SLUG> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_SLUG",
            help = "Space slug for the generated sample-data space"
        )]
        space_id: String,
        #[arg(
            long,
            help = "Sample-data scenario ID (run `ugoite space sample-scenarios` to list options)"
        )]
        scenario: Option<String>,
        #[arg(
            long,
            default_value_t = 50,
            help = "Approximate number of generated entries for the seeded space"
        )]
        entry_count: usize,
        #[arg(long, help = "Deterministic random seed for reproducible sample data")]
        seed: Option<u64>,
        /// Create a portable owner principal with this display name.
        /// A node binding is still required before remote access.
        #[arg(long)]
        owner: Option<String>,
    },
    /// List sample scenarios
    SampleScenarios,
    /// Create a sample data job
    SampleJob {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_SLUG> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_SLUG",
            help = "Space slug for the generated sample-data space"
        )]
        space_id: String,
        #[arg(
            long,
            help = "Sample-data scenario ID (run `ugoite space sample-scenarios` to list options)"
        )]
        scenario: Option<String>,
        #[arg(
            long,
            default_value_t = 50,
            help = "Approximate number of generated entries for the seeded space"
        )]
        entry_count: usize,
        #[arg(long, help = "Deterministic random seed for reproducible sample data")]
        seed: Option<u64>,
        /// Create a portable owner principal with this display name.
        /// A node binding is still required before remote access.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Get sample data job status
    SampleJobStatus {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root that stores sample-data job state"
        )]
        root_path: String,
        #[arg(help = "Job ID returned by `ugoite space sample-job`")]
        job_id: String,
    },
    /// Test storage connection
    TestConnection { storage_config_json: String },
    /// List space members (backend/api mode only)
    Members {
        #[arg(
            value_name = "SPACE_UID",
            help = "Immutable Space UID in backend/api mode."
        )]
        space_path: String,
    },
    /// List Space audit events (append-only evidence: event/change/revision/actor only, never paths or secrets)
    AuditEvents {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(
            long,
            default_value_t = 50,
            help = "Maximum audit events to return. Effective range is 1..=500 after normalization: 0 normalizes to 1 and larger values clamp to 500."
        )]
        limit: u64,
    },
}

fn require_local_root<'a>(root_path: Option<&'a str>, command_name: &str) -> Result<&'a str> {
    root_path
        .ok_or_else(|| anyhow::anyhow!("{command_name} requires --root <LOCAL_ROOT> in core mode"))
}

fn require_space_list_root(root_path: Option<&str>) -> Result<String> {
    root_path
        .map(normalize_space_root)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "space list requires ROOT_PATH as /path/to/root or /path/to/root/spaces in core mode"
            )
        })
}

fn resolve_sample_owner_display_name(owner: Option<String>) -> Option<String> {
    match owner {
        Some(owner_display_name) => {
            let owner_display_name = owner_display_name.trim().to_string();
            (!owner_display_name.is_empty()).then_some(owner_display_name)
        }
        None => None,
    }
}

fn validate_patch_settings(settings: &serde_json::Value) -> Result<()> {
    let patch = serde_json::json!({ "settings": settings });
    validate_public_space_patch(&patch).map_err(|error| anyhow::anyhow!(error.to_string()))
}

/// Concise TTY projection for Space audit event rows. Piped output keeps
/// full JSON. Only identity fields are projected; paths and secrets never
/// enter audit events by construction.
fn audit_rows_table(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let metadata = row.get("metadata");
            let actor = row
                .get("actor_principal_id")
                .or_else(|| row.get("subject_principal_id"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let target_type = row
                .get("target_type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let target_id = row
                .get("target_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            serde_json::json!({
                "event_id": row.get("event_id").and_then(|value| value.as_str()).unwrap_or_default(),
                "action": row.get("action").and_then(|value| value.as_str()).unwrap_or_default(),
                "actor": actor,
                "target": if target_type.is_empty() { target_id.to_owned() } else { format!("{target_type}:{target_id}") },
                "revision_id": metadata.and_then(|meta| meta.get("revision_id")).and_then(|value| value.as_str()).unwrap_or_default(),
            })
        })
        .collect()
}

/// Single `space create` display-name rule.
///
/// An absent name defaults to the requested slug; a provided name is trimmed
/// via the shared domain normalization and an empty/whitespace-only value
/// fails before any write. The positional slug stays the stable Space key;
/// the resolved name only seeds the durable display name on first creation.
fn resolve_create_display_name(requested_slug: &str, display_name: Option<&str>) -> Result<String> {
    match display_name {
        None => Ok(requested_slug.to_string()),
        Some(name) => ugoite_domain::space::normalize_space_display_name(name)
            .map_err(|error| anyhow::anyhow!(error.to_string())),
    }
}

pub async fn create_space_cmd(
    root_path: Option<&str>,
    space_id: &str,
    command_name: &str,
) -> Result<()> {
    create_space_cmd_with_name(root_path, space_id, None, command_name).await
}

pub async fn create_space_cmd_with_name(
    root_path: Option<&str>,
    space_id: &str,
    display_name: Option<&str>,
    command_name: &str,
) -> Result<()> {
    let config = load_config()?;
    let requested_slug = parse_space_path(space_id).1;
    let resolved_name = resolve_create_display_name(&requested_slug, display_name)?;
    if let Some(base) = validated_base_url(&config)? {
        // Remote Space creation may require fresh human presence; the
        // step-up handoff (browser approval, one automatic retry) keeps the
        // ceremony policy intact instead of weakening it.
        let result = step_up::execute_with_step_up(
            &base,
            "space.create",
            serde_json::json!({}),
            Some(serde_json::json!({"slug": requested_slug, "name": resolved_name})),
            None,
        )
        .await?;
        print_json(&result);
        return Ok(());
    }
    let root_path = require_local_root(root_path, command_name)?;
    let service = UgoiteService::new_without_background_refresh(root_path)?;
    let outcome = service
        .ensure_operator_space_with_name(&requested_slug, &resolved_name)
        .await?;
    print_json(
        &serde_json::json!({"created": outcome.created(), "id": outcome.space_id(), "slug": requested_slug, "name": resolved_name}),
    );
    Ok(())
}

/// Canonical path triggers: explicit connection selection, explicit opt-out,
/// an explicit `--config` file, or any existing canonical source. Otherwise
/// the legacy positional-path behavior is preserved unchanged.
fn should_use_canonical_create(
    explicit_config: Option<&std::path::Path>,
    explicit_connection: Option<&str>,
    no_context: bool,
) -> bool {
    if explicit_connection.is_some() || no_context || explicit_config.is_some() {
        return true;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    !crate::cli_config::discover::source_stack_from_environment(None, &cwd).is_empty()
}

/// Select the connection for `space create` (plan section 37):
/// explicit `--connection` → override/current context's connection → the only
/// defined connection → error (never guess among several). A dangling
/// current_context (name without a context entry) fails closed instead of
/// falling through, so a broken config can never silently pick a connection.
fn select_create_connection(
    effective: &crate::cli_config::EffectiveConfig,
    explicit: Option<&str>,
    context_override: Option<&str>,
) -> Result<String> {
    if let Some(name) = explicit {
        if !effective.connections.contains_key(name) {
            bail!("Connection {name:?} is not defined.");
        }
        return Ok(name.to_string());
    }
    if let Some(name) = context_override {
        let context = effective
            .contexts
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Context {name:?} is not defined."))?;
        return Ok(context.value.connection.clone());
    }
    if let Some(current) = effective.current_context.as_ref() {
        let context = effective.contexts.get(&current.value).ok_or_else(|| {
            anyhow::anyhow!(
                "Current context {:?} is not defined. Select another context with `ugoite context use <NAME>`.",
                current.value
            )
        })?;
        return Ok(context.value.connection.clone());
    }
    if effective.connections.len() == 1 {
        if let Some(name) = effective.connections.keys().next() {
            return Ok(name.clone());
        }
    }
    bail!("Cannot determine a connection for `space create`: pass --connection <NAME>.")
}

/// Canonical `space create`: create the Space, then register it as a CLI
/// context (immutable UID) and make it current — unless `--no-context`.
/// A config write failure after successful creation never deletes the Space;
/// it fails non-zero with the Space UID and config path instead (plan 35).
#[allow(clippy::too_many_arguments)]
async fn create_space_canonical(
    space_path: &str,
    display_name: Option<&str>,
    explicit_connection: Option<&str>,
    no_context: bool,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
    fmt: Format,
) -> Result<()> {
    use crate::cli_config::{
        load_cli_config, mutate_write_target, unique_context_name, ConnectionConfig, ContextConfig,
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let files = load_cli_config(explicit_config, &cwd)?;
    let connection_name =
        select_create_connection(&files.effective, explicit_connection, context_override)?;
    let connection = files
        .effective
        .connections
        .get(&connection_name)
        .ok_or_else(|| anyhow::anyhow!("Connection {connection_name:?} is not defined."))?;
    let requested_slug = parse_space_path(space_path).1;
    if requested_slug.trim().is_empty() {
        bail!("Space slug must not be empty");
    }
    if space_path.contains("/spaces/") || space_path.contains('/') {
        // Canonical core creation ignores any positional root: the
        // connection's root is the authority. Warn instead of silently
        // using a different directory than the user typed.
        eprintln!(
            "Note: canonical `space create` uses connection {connection_name:?} root; the positional path is read as slug {requested_slug:?}."
        );
    }
    let resolved_name = resolve_create_display_name(&requested_slug, display_name)?;

    // Create the Space first (Knowledge mutation), before any config change.
    enum Created {
        Core { space_uid: uuid::Uuid },
        Remote { space_uid: uuid::Uuid },
    }
    let created = match &connection.value {
        ConnectionConfig::Core { root } => {
            let service = UgoiteService::new_without_background_refresh(root)?;
            let outcome = service
                .ensure_operator_space_with_name(&requested_slug, &resolved_name)
                .await?;
            Created::Core {
                space_uid: outcome.space_id(),
            }
        }
        ConnectionConfig::Backend { url } | ConnectionConfig::Api { url } => {
            let parsed =
                crate::cli_config::model::validate_remote_url(url, "Space creation endpoint")?;
            let base = parsed.as_str().trim_end_matches('/').to_string();
            let result = step_up::execute_with_step_up(
                &base,
                "space.create",
                serde_json::json!({}),
                Some(serde_json::json!({"slug": requested_slug, "name": resolved_name})),
                None,
            )
            .await?;
            let uid_text = result
                .get("space_uid")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Server response omitted the new Space UID"))?;
            let space_uid = uuid::Uuid::parse_str(uid_text)
                .map_err(|_| anyhow::anyhow!("Server returned an invalid Space UID"))?;
            if space_uid.get_version() != Some(uuid::Version::SortRand) {
                bail!("Server returned a non-UUIDv7 Space UID");
            }
            Created::Remote { space_uid }
        }
    };
    let space_uid = match created {
        Created::Core { space_uid } | Created::Remote { space_uid } => space_uid,
    };

    if no_context {
        emit_create_output(fmt, &requested_slug, &space_uid, &connection_name, None);
        return Ok(());
    }

    // Propagate the credential only when the override/current context already
    // scopes the same connection; never invent one. An explicit --context
    // selects the connection but never mutates current_context by itself.
    let scope_context = context_override
        .and_then(|name| files.effective.contexts.get(name))
        .or_else(|| {
            files
                .effective
                .current_context
                .as_ref()
                .and_then(|current| files.effective.contexts.get(&current.value))
        });
    let credential = scope_context
        .filter(|context| context.value.connection == connection_name)
        .and_then(|context| context.value.credential.clone());
    let context_name = unique_context_name(&files.effective, &connection_name, &requested_slug);
    let value = ContextConfig {
        connection: connection_name.clone(),
        space_uid,
        credential,
    };
    let write_target = files.write_target.clone();
    let name_for_closure = context_name.clone();
    let value_for_closure = value.clone();
    let write_result = mutate_write_target(&files, |config| {
        config
            .contexts
            .insert(name_for_closure.clone(), value_for_closure.clone());
        config.current_context = Some(name_for_closure.clone());
    });
    match write_result {
        Ok(target) => {
            emit_create_output(
                fmt,
                &requested_slug,
                &space_uid,
                &connection_name,
                Some((&context_name, &target)),
            );
            Ok(())
        }
        Err(error) => {
            // The Space already exists; never roll it back for a config
            // failure. Report partial success with a non-zero exit.
            let uid_view = serde_json::json!({ "space_uid": space_uid });
            bail!(
                "Space created successfully, but CLI context could not be saved.\nSpace:\n  {requested_slug}\n  {}\nConfig:\n  {}\nThe Space was not deleted.\nCause: {error}",
                uid_view["space_uid"].as_str().unwrap_or_default(),
                write_target.display(),
            );
        }
    }
}

/// Human + structured output for canonical creation (plan sections 38-39).
/// UIDs pass through the JSON-value output boundary used everywhere else.
fn emit_create_output(
    fmt: Format,
    slug: &str,
    space_uid: &uuid::Uuid,
    connection_name: &str,
    registered: Option<(&str, &std::path::Path)>,
) {
    let uid_view = serde_json::json!({ "space_uid": space_uid });
    let uid_text = uid_view["space_uid"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if fmt == Format::Json {
        match registered {
            Some((context_name, target)) => print_json(&serde_json::json!({
                "space": { "space_uid": uid_text, "slug": slug },
                "connection": connection_name,
                "context": {
                    "created": true,
                    "name": context_name,
                    "current": true,
                    "config_path": target.to_string_lossy(),
                },
            })),
            None => print_json(&serde_json::json!({
                "space": { "space_uid": uid_text, "slug": slug },
                "connection": connection_name,
                "context": { "created": false, "reason": "disabled" },
            })),
        }
        return;
    }
    println!("Created Space {slug:?}");
    println!("  uid: {uid_text}");
    println!("  connection: {connection_name}");
    match registered {
        Some((context_name, target)) => {
            println!("Added CLI context");
            println!("  context: {context_name}");
            println!("  config: {}", target.display());
            println!("  current: yes");
        }
        None => {
            println!("CLI context registration skipped (--no-context)");
        }
    }
}

pub async fn run(
    cmd: SpaceCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let config = load_config()?;
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SpaceSubCmd::Create {
            space_path,
            name,
            connection,
            no_context,
        } => {
            if should_use_canonical_create(explicit_config, connection.as_deref(), no_context) {
                create_space_canonical(
                    &space_path,
                    name.as_deref(),
                    connection.as_deref(),
                    no_context,
                    explicit_config,
                    context_override,
                    fmt,
                )
                .await?;
                return Ok(());
            }
            if let Some(base) = validated_base_url(&config)? {
                // Backend/api creation takes a new human-readable slug; the
                // server-generated Space UID in the response is the authority
                // for all later operations. Never treat the requested slug as
                // a UID and never fall back to another Space.
                let requested_slug = parse_space_path(&space_path).1;
                let resolved_name = resolve_create_display_name(&requested_slug, name.as_deref())?;
                let result = step_up::execute_with_step_up(
                    &base,
                    "space.create",
                    serde_json::json!({}),
                    Some(serde_json::json!({"slug": requested_slug, "name": resolved_name})),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let requested_slug = parse_space_path(&space_path).1;
            let resolved_name = resolve_create_display_name(&requested_slug, name.as_deref())?;
            let (root, _) = resolve_space_reference(&config, &space_path, "space create")?;
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let outcome = service
                .ensure_operator_space_with_name(&requested_slug, &resolved_name)
                .await?;
            print_json(
                &serde_json::json!({"created": outcome.created(), "id": outcome.space_id(), "slug": requested_slug, "name": resolved_name}),
            );
        }
        SpaceSubCmd::List { root_path } => {
            if let Some(base) = validated_base_url(&config)? {
                let result =
                    http::execute(&base, "space.list", serde_json::json!({}), None).await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("SPACE_UID", "space_uid"), ("NAME", "name")]);
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let root_path = require_space_list_root(root_path.as_deref())?;
            let service = UgoiteService::new_without_background_refresh(&root_path)?;
            let spaces = service.list_space_ids().await?;
            if fmt != Format::Json {
                let paths = spaces
                    .iter()
                    .map(|space_id| {
                        Path::new(&root_path)
                            .join(service.workspace_path(space_id))
                            .display()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                print_list_table("LOCAL_SPACE_PATH", &paths);
            } else {
                print_json(&spaces);
            }
        }
        SpaceSubCmd::Get { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "space get")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.get",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let space = service.get_space(&space_id).await?;
            print_json(&space);
        }
        SpaceSubCmd::Patch {
            space_path,
            name,
            storage_config,
            settings,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "space patch")?;
            let mut patch = serde_json::Map::new();
            if let Some(n) = name {
                patch.insert("name".to_string(), serde_json::json!(n));
            }
            if let Some(s) = &storage_config {
                let v: serde_json::Value = serde_json::from_str(s)?;
                patch.insert("storage_config".to_string(), v);
            }
            if let Some(s) = &settings {
                let v: serde_json::Value = serde_json::from_str(s)?;
                validate_patch_settings(&v)?;
                patch.insert("settings".to_string(), v);
            }
            if let Some(base) = validated_base_url(&config)? {
                let result = step_up::execute_with_step_up(
                    &base,
                    "space.patch",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::Value::Object(patch)),
                    Some(space_id.as_str()),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .patch_space(&space_id, &serde_json::Value::Object(patch))
                .await?;
            print_json(&result);
        }
        SpaceSubCmd::SampleData {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
            owner,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
                owner_display_name: resolve_sample_owner_display_name(owner),
            };
            let summary = ugoite_iceberg::sample_data::create_sample_space_with_terminal_progress(
                &op, &root_uri, &opts,
            )
            .await?;
            print_json(&serde_json::json!({
                "created": true,
                "id": summary.space_id,
                "slug": space_id,
                "scenario": summary.scenario,
                "entry_count": summary.entry_count,
                "form_count": summary.form_count,
                "forms": summary.forms,
            }));
        }
        SpaceSubCmd::SampleScenarios => {
            let scenarios = ugoite_iceberg::sample_data::list_sample_scenarios();
            print_json(&scenarios);
        }
        SpaceSubCmd::SampleJob {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
            owner,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
                owner_display_name: resolve_sample_owner_display_name(owner),
            };
            let job = ugoite_iceberg::sample_data::create_sample_space_job_and_wait(
                &op, &root_uri, &opts,
            )
            .await?;
            print_json(&job);
        }
        SpaceSubCmd::SampleJobStatus { root_path, job_id } => {
            let op = operator_for_path(&root_path)?;
            let job = ugoite_iceberg::sample_data::get_sample_space_job(&op, &job_id).await?;
            let v = serde_json::to_value(job)?;
            print_json(&v);
        }
        SpaceSubCmd::TestConnection {
            storage_config_json,
        } => {
            let payload: serde_json::Value = serde_json::from_str(&storage_config_json)?;
            let result = ugoite_iceberg::service::probe_storage_connection(
                &ugoite_iceberg::space::StorageConnectionTestConfig::from_payload(&payload)?,
            )
            .await?;
            print_json(&result);
        }
        SpaceSubCmd::Members { space_path } => {
            let space_id = resolve_backend_space_uid(&space_path, "space members")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.members.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            bail!("{}", backend_api_mode_error(&config, "members"));
        }
        SpaceSubCmd::AuditEvents {
            space_path,
            offset,
            limit,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "space audit-events")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.audit",
                    serde_json::json!({"space_id": space_id, "offset": offset, "limit": limit}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(rows) = result.get("items").and_then(|value| value.as_array()) {
                        let table = audit_rows_table(rows);
                        print_json_table(
                            &table,
                            &[
                                ("EVENT_ID", "event_id"),
                                ("ACTION", "action"),
                                ("ACTOR", "actor"),
                                ("TARGET", "target"),
                                ("REVISION", "revision_id"),
                            ],
                        );
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            // Open hook heals crash-missing evidence; the list itself is a
            // light read of committed evidence.
            service.open_space(&space_id).await?;
            // Clamp before `usize` conversion: effective range 1..=500, so
            // even the largest CLI integer cannot overflow before the cap.
            let (audit_limit, audit_offset) =
                ugoite_iceberg::audit::normalize_audit_page(limit, offset);
            let result = service
                .list_space_audit(&space_id, audit_offset, audit_limit)
                .await?;
            if fmt != Format::Json {
                if let Some(rows) = result.get("items").and_then(|value| value.as_array()) {
                    let table = audit_rows_table(rows);
                    print_json_table(
                        &table,
                        &[
                            ("EVENT_ID", "event_id"),
                            ("ACTION", "action"),
                            ("ACTOR", "actor"),
                            ("TARGET", "target"),
                            ("REVISION", "revision_id"),
                        ],
                    );
                    return Ok(());
                }
            }
            print_json(&result);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_create_display_name;

    #[test]
    fn create_display_name_defaults_to_requested_slug() {
        assert_eq!(
            resolve_create_display_name("team-notes", None).unwrap(),
            "team-notes"
        );
    }

    #[test]
    fn create_display_name_trims_provided_name() {
        assert_eq!(
            resolve_create_display_name("team-notes", Some("  Team Notes  ")).unwrap(),
            "Team Notes"
        );
    }

    #[test]
    fn create_display_name_rejects_whitespace_only_before_any_write() {
        for rejected in ["", "   ", "\t\n "] {
            resolve_create_display_name("team-notes", Some(rejected))
                .expect_err("whitespace-only display name must fail before any write");
        }
    }
}
