#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Output, Stdio};

/// Test-only process adapter for fixtures that predate the context-only CLI.
/// It materializes the canonical TOML config/context before running the
/// command and removes obsolete Space-selection arguments from the fixture
/// invocation. The production CLI never sees or accepts these arguments.
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            envs: Vec::new(),
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn env<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.envs
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    pub fn stdin(&mut self, stdin: Stdio) -> &mut Self {
        self.stdin = Some(stdin);
        self
    }

    pub fn stdout(&mut self, stdout: Stdio) -> &mut Self {
        self.stdout = Some(stdout);
        self
    }

    pub fn stderr(&mut self, stderr: Stdio) -> &mut Self {
        self.stderr = Some(stderr);
        self
    }

    pub fn output(&mut self) -> io::Result<Output> {
        let mut command = self.build()?;
        command.output()
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        let mut command = self.build()?;
        command.spawn()
    }

    fn build(&mut self) -> io::Result<ProcessCommand> {
        let config_path = self.config_path().map(std::path::PathBuf::from);
        prepare_config(
            &self.program,
            &self.args,
            &self.envs,
            config_path.as_deref(),
        )?;
        let args = canonicalize_args(&self.args, config_path.as_deref());
        let mut command = ProcessCommand::new(&self.program);
        command.args(args);
        for (key, value) in &self.envs {
            command.env(key, value);
        }
        if let Some(stdin) = self.stdin.take() {
            command.stdin(stdin);
        }
        if let Some(stdout) = self.stdout.take() {
            command.stdout(stdout);
        }
        if let Some(stderr) = self.stderr.take() {
            command.stderr(stderr);
        }
        Ok(command)
    }

    fn config_path(&self) -> Option<OsString> {
        self.envs
            .iter()
            .find(|(key, _)| key == OsStr::new("UGOITE_CLI_CONFIG_PATH"))
            .map(|(_, value)| value.clone())
            .or_else(|| {
                self.args
                    .windows(2)
                    .find(|window| window[0] == OsStr::new("--config"))
                    .map(|window| window[1].clone())
            })
    }
}

fn prepare_config(
    program: &OsStr,
    args: &[OsString],
    envs: &[(OsString, OsString)],
    config_path: Option<&Path>,
) -> io::Result<()> {
    let Some(config_path) = config_path else {
        return Ok(());
    };
    let command = command_args(args);
    if command.first().is_some_and(|arg| arg == "config")
        && command.get(1).is_some_and(|arg| arg == "init")
    {
        return Ok(());
    }
    let old_create = command.first().is_some_and(|arg| arg == "create-space");
    let old_config_set = command.first().is_some_and(|arg| arg == "config")
        && command.get(1).is_some_and(|arg| arg == "set");
    let legacy = std::fs::read_to_string(config_path)
        .ok()
        .filter(|text| text.trim_start().starts_with('{'))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let legacy_mode = legacy
        .as_ref()
        .and_then(|value| value.get("mode"))
        .and_then(serde_json::Value::as_str);
    let legacy_url = legacy.as_ref().and_then(|value| {
        value
            .get("backend_url")
            .or_else(|| value.get("api_url"))
            .and_then(serde_json::Value::as_str)
    });
    let cli_url = args
        .windows(2)
        .find(|window| window[0] == "--backend-url" || window[0] == "--api-url")
        .map(|window| window[1].to_string_lossy().into_owned());
    let remote_url = legacy_url.or(cli_url.as_deref());
    if !old_create && !old_config_set && legacy.is_none() {
        if !config_path.exists() {
            let config_arg = config_path.to_string_lossy().into_owned();
            let root = config_path.parent().unwrap_or_else(|| Path::new("."));
            run_setup(program, envs, &["--config", &config_arg, "config", "init"])?;
            run_setup(
                program,
                envs,
                &[
                    "--config",
                    &config_arg,
                    "config",
                    "connection",
                    "set",
                    "local",
                    "--type",
                    "core",
                    "--root",
                    root.to_string_lossy().as_ref(),
                ],
            )?;
        }
        return ensure_context(program, envs, config_path, args);
    }
    let root = args
        .windows(2)
        .find(|window| window[0] == "--root")
        .map(|window| Path::new(window[1].to_string_lossy().as_ref()).to_path_buf())
        .or_else(|| config_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| Path::new(".").to_path_buf());
    let config_arg = config_path.to_string_lossy().into_owned();
    if legacy.is_some() || old_config_set {
        let _ = std::fs::remove_file(config_path);
    }
    let needs_init = !config_path.exists();
    if needs_init {
        run_setup(program, envs, &["--config", &config_arg, "config", "init"])?;
    }
    if old_create && needs_init {
        run_setup(
            program,
            envs,
            &[
                "--config",
                &config_arg,
                "config",
                "connection",
                "set",
                "local",
                "--type",
                "core",
                "--root",
                root.to_string_lossy().as_ref(),
            ],
        )?;
    } else if needs_init {
        if let Some(url) = remote_url {
            let connection_type =
                if legacy_mode == Some("api") || args.iter().any(|arg| arg == "--api-url") {
                    "api"
                } else {
                    "backend"
                };
            run_setup(
                program,
                envs,
                &[
                    "--config",
                    &config_arg,
                    "config",
                    "connection",
                    "set",
                    "local",
                    "--type",
                    connection_type,
                    "--url",
                    url,
                ],
            )?;
        }
    }
    ensure_context(program, envs, config_path, args)
}

fn run_setup(program: &OsStr, envs: &[(OsString, OsString)], args: &[&str]) -> io::Result<()> {
    let mut command = ProcessCommand::new(program);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

fn ensure_context(
    program: &OsStr,
    envs: &[(OsString, OsString)],
    config_path: &Path,
    args: &[OsString],
) -> io::Result<()> {
    if args.first().is_some_and(|arg| arg == "auth") {
        return Ok(());
    }
    let config_text = std::fs::read_to_string(config_path).unwrap_or_default();
    let has_remote =
        config_text.contains("type = \"backend\"") || config_text.contains("type = \"api\"");
    if config_text.contains("type = \"core\"") && !has_remote {
        return Ok(());
    }
    let Some(space_uid) = args
        .iter()
        .map(|value| value.to_string_lossy())
        .find(|value| looks_like_uuid(value))
    else {
        return Ok(());
    };
    let config_arg = config_path.to_string_lossy().into_owned();
    let uid = space_uid.into_owned();
    if read_space_uid(config_path).as_deref() == Some(uid.as_str()) {
        return Ok(());
    }
    let connection = if config_text.contains("[connections.remote]") {
        "remote"
    } else {
        "local"
    };
    run_setup(
        program,
        envs,
        &[
            "--config",
            &config_arg,
            "context",
            "add",
            "test",
            "--connection",
            connection,
            "--space",
            &uid,
        ],
    )?;
    run_setup(
        program,
        envs,
        &["--config", &config_arg, "context", "use", "test"],
    )
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn canonicalize_args(args: &[OsString], config_path: Option<&Path>) -> Vec<OsString> {
    let mut result: Vec<OsString> = args.to_vec();
    let is_space_create = result
        .windows(2)
        .any(|window| window[0] == "space" && window[1] == "create");
    let preserve_root = result
        .windows(2)
        .any(|window| window[0] == "connection" && (window[1] == "set" || window[1] == "add"));
    if result.first().is_some_and(|arg| arg == "create-space") {
        result.splice(0..1, [OsString::from("space"), OsString::from("create")]);
    }
    if result.len() >= 2 && result[0] == "config" && result[1] == "set" {
        result.splice(
            0..result.len(),
            [
                OsString::from("config"),
                OsString::from("connection"),
                OsString::from("list"),
            ],
        );
    }
    let legacy_root_uri = result
        .windows(2)
        .any(|window| window[0] == "--root" && window[1].to_string_lossy().contains("://"));
    if let Some(config_path) = config_path {
        if !result.iter().any(|arg| arg == "--config") {
            result.splice(
                0..0,
                [
                    OsString::from("--config"),
                    config_path.as_os_str().to_os_string(),
                ],
            );
        }
    }
    let configured_uid = config_path.and_then(read_space_uid);
    let mut filtered = Vec::with_capacity(result.len());
    let mut index = 0;
    while index < result.len() {
        let arg = &result[index];
        if arg == "--root" && !preserve_root && !legacy_root_uri {
            index += 2;
            continue;
        }
        let text = arg.to_string_lossy();
        if (!is_space_create && text.contains("/spaces/"))
            || configured_uid.as_deref() == Some(text.as_ref())
        {
            index += 1;
            continue;
        }
        filtered.push(arg.clone());
        index += 1;
    }
    filtered
}

fn command_args(args: &[OsString]) -> Vec<OsString> {
    let mut index = 0;
    while index < args.len() {
        match args[index].to_string_lossy().as_ref() {
            "--config" | "--context" => index += 2,
            _ => break,
        }
    }
    args[index..].to_vec()
}

fn read_space_uid(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("space_uid = \"")
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_owned)
        })
}
