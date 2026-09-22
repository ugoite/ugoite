use crate::http;
use crate::output::{effective_format, print_json, print_json_table, print_list_table, Format};
use crate::step_up;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::Path;
use ugoite_iceberg::service::{validate_public_space_patch, UgoiteService};

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
        long_about = "Create a new Space on a named connection.\n\nThe new Space is automatically registered as the current context by its immutable Space UID. Use --no-context to skip registration, or --connection to select a named connection explicitly.\n\nExamples:\n  ugoite config init\n  ugoite space create demo\n  ugoite space create team-notes --connection remote --name \"Team Notes\""
    )]
    Create {
        #[arg(value_name = "SPACE_SLUG", help = "New human-readable Space slug.")]
        space_slug: String,
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
        long_about = "List all spaces from the selected connection.\n\nUse `ugoite context use NAME` or `--context NAME` to select a connection context. The selected connection's configured root is used."
    )]
    List {},
    /// Get space metadata
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Get,
    /// Patch space metadata
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Patch {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        storage_config: Option<String>,
        #[arg(long)]
        settings: Option<String>,
    },
    /// List sample scenarios
    SampleScenarios,
    /// Test storage connection
    TestConnection { storage_config_json: String },
    /// List space members (remote backend/api connections only)
    Members,
    /// List Space audit events (append-only evidence: event/change/revision/actor only, never paths or secrets)
    AuditEvents {
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

fn select_connection_credential(
    effective: &crate::cli_config::EffectiveConfig,
    connection_name: &str,
    context_override: Option<&str>,
) -> Result<Option<String>> {
    let scope_context = context_override
        .and_then(|name| effective.contexts.get(name))
        .or_else(|| {
            effective
                .current_context
                .as_ref()
                .and_then(|current| effective.contexts.get(&current.value))
        });
    let store = crate::cli_config::credentials::load_credentials()?;
    crate::cli_config::credentials::resolve_credential_for_connection(
        &store,
        connection_name,
        None,
        scope_context.and_then(|context| context.value.credential.as_deref()),
        scope_context.map(|context| context.value.connection.as_str()),
    )
}

/// Canonical `space create`: create the Space, then register it as a CLI
/// context (immutable UID) and make it current — unless `--no-context`.
/// A config write failure after successful creation never deletes the Space;
/// it fails non-zero with the Space UID and config path instead (plan 35).
#[allow(clippy::too_many_arguments)]
async fn create_space_canonical(
    space_slug: &str,
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
    let credential =
        select_connection_credential(&files.effective, &connection_name, context_override)?;
    let connection = files
        .effective
        .connections
        .get(&connection_name)
        .ok_or_else(|| anyhow::anyhow!("Connection {connection_name:?} is not defined."))?;
    ugoite_domain::id::validate_space_id(space_slug)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let requested_slug = space_slug.to_string();
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
            let result = step_up::execute_with_step_up_for_connection(
                &base,
                &connection_name,
                credential.as_deref(),
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
    let credential = credential.or_else(|| {
        scope_context
            .filter(|context| context.value.connection == connection_name)
            .and_then(|context| context.value.credential.clone())
    });
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
    // UIDs render through the JSON-value output boundary used by
    // `context list/get`, `space list`, and `config current`: the Space UID
    // is a non-secret immutable identifier, and the value boundary keeps
    // every UID display on the single established output path.
    let uid_view = serde_json::json!({ "space_uid": space_uid });
    if fmt == Format::Json {
        match registered {
            Some((context_name, target)) => print_json(&serde_json::json!({
                "space": { "space_uid": uid_view["space_uid"].clone(), "slug": slug },
                "connection": connection_name,
                "context": {
                    "created": true,
                    "name": context_name,
                    "current": true,
                    "config_path": target.to_string_lossy(),
                },
            })),
            None => print_json(&serde_json::json!({
                "space": { "space_uid": uid_view["space_uid"].clone(), "slug": slug },
                "connection": connection_name,
                "context": { "created": false, "reason": "disabled" },
            })),
        }
        return;
    }
    println!("Created Space {slug:?}");
    println!(
        "  uid: {}",
        uid_view["space_uid"].as_str().unwrap_or_default()
    );
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
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SpaceSubCmd::Create {
            space_slug,
            name,
            connection,
            no_context,
        } => {
            create_space_canonical(
                &space_slug,
                name.as_deref(),
                connection.as_deref(),
                no_context,
                explicit_config,
                context_override,
                fmt,
            )
            .await?;
        }
        SpaceSubCmd::List {} => {
            use crate::cli_config::{load_cli_config, ConnectionConfig};
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let files = load_cli_config(explicit_config, &cwd)?;
            let connection_name =
                select_create_connection(&files.effective, None, context_override)?;
            let connection = files
                .effective
                .connections
                .get(&connection_name)
                .ok_or_else(|| anyhow::anyhow!("Connection {connection_name:?} is not defined."))?;
            let root_path = match &connection.value {
                ConnectionConfig::Core { root } => root.clone(),
                ConnectionConfig::Backend { url } | ConnectionConfig::Api { url } => {
                    let base =
                        crate::cli_config::model::validate_remote_url(url, "Space list endpoint")?
                            .as_str()
                            .trim_end_matches('/')
                            .to_string();
                    let credential = select_connection_credential(
                        &files.effective,
                        &connection_name,
                        context_override,
                    )?;
                    let result = http::execute_for_connection(
                        &base,
                        &connection_name,
                        credential.as_deref(),
                        "space.list",
                        serde_json::json!({}),
                        None,
                    )
                    .await?;
                    if fmt != Format::Json {
                        if let Some(arr) = result.as_array() {
                            print_json_table(arr, &[("SPACE_UID", "space_uid"), ("NAME", "name")]);
                            return Ok(());
                        }
                    }
                    print_json(&result);
                    return Ok(());
                }
            };
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
        SpaceSubCmd::Get => {
            let target = crate::cli_config::resolve_command_target(
                explicit_config,
                context_override,
                "space get",
            )?;
            match &target {
                crate::cli_config::SpaceTarget::Remote { ref space_uid, .. } => {
                    let result = http::execute_for_target(
                        &target,
                        "space.get",
                        serde_json::json!({"space_id": space_uid}),
                        None,
                    )
                    .await?;
                    print_json(&result);
                }
                crate::cli_config::SpaceTarget::Core { root, space_id } => {
                    let service = UgoiteService::new_without_background_refresh(root)?;
                    let space = service.get_space(space_id).await?;
                    print_json(&space);
                }
            }
        }
        SpaceSubCmd::Patch {
            name,
            storage_config,
            settings,
        } => {
            let target = crate::cli_config::resolve_command_target(
                explicit_config,
                context_override,
                "space patch",
            )?;
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
            match target {
                crate::cli_config::SpaceTarget::Remote { ref space_uid, .. } => {
                    let result = step_up::execute_with_step_up_for_target(
                        &target,
                        "space.patch",
                        serde_json::json!({"space_id": space_uid}),
                        Some(serde_json::Value::Object(patch)),
                        Some(space_uid.as_str()),
                    )
                    .await?;
                    print_json(&result);
                }
                crate::cli_config::SpaceTarget::Core { root, space_id } => {
                    let service = UgoiteService::new_without_background_refresh(&root)?;
                    let result = service
                        .patch_space(&space_id, &serde_json::Value::Object(patch))
                        .await?;
                    print_json(&result);
                }
            }
        }
        SpaceSubCmd::SampleScenarios => {
            let scenarios = ugoite_iceberg::sample_data::list_sample_scenarios();
            print_json(&scenarios);
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
        SpaceSubCmd::Members => {
            let target = crate::cli_config::resolve_command_target(
                explicit_config,
                context_override,
                "space members",
            )?;
            let crate::cli_config::SpaceTarget::Remote { space_uid, .. } = &target else {
                bail!("space members requires a backend or api context");
            };
            let result = http::execute_for_target(
                &target,
                "space.members.list",
                serde_json::json!({"space_id": space_uid}),
                None,
            )
            .await?;
            print_json(&result);
        }
        SpaceSubCmd::AuditEvents { offset, limit } => {
            let target = crate::cli_config::resolve_command_target(
                explicit_config,
                context_override,
                "space audit-events",
            )?;
            if let crate::cli_config::SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "space.audit",
                    serde_json::json!({"space_id": space_uid, "offset": offset, "limit": limit}),
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
            let crate::cli_config::SpaceTarget::Core { root, space_id } = &target else {
                bail!("operation space.audit does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            // Open hook heals crash-missing evidence; the list itself is a
            // light read of committed evidence.
            service.open_space(space_id).await?;
            // Clamp before `usize` conversion: effective range 1..=500, so
            // even the largest CLI integer cannot overflow before the cap.
            let (audit_limit, audit_offset) =
                ugoite_iceberg::audit::normalize_audit_page(limit, offset);
            let result = service
                .list_space_audit(space_id, audit_offset, audit_limit)
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
