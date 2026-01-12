use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasmtime_wasi::preview1;
use wasmtime_wasi::{AsyncStdinStream, AsyncStdoutStream, WasiCtxBuilder};

const MAGIC_HOST: &[u8; 6] = b"\0HOST\0";
const MAGIC_RESULT: &[u8; 6] = b"\0RSLT\0";
const MAGIC_ERROR: &[u8; 6] = b"\0ERRR\0";

pub type HostCallHandler = dyn Fn(&str, &str, Option<Value>) -> Result<Value> + Send + Sync;

#[derive(thiserror::Error, Debug)]
pub enum SandboxError {
    #[error("sandbox.wasm is missing at {0}")]
    MissingWasm(PathBuf),

    #[error("sandbox execution error: {0}")]
    Execution(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

async fn read_u32_be(reader: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .await
        .context("failed to read length")?;
    Ok(u32::from_be_bytes(buf))
}

async fn write_u32_be(writer: &mut (impl tokio::io::AsyncWrite + Unpin), v: u32) -> Result<()> {
    writer
        .write_all(&v.to_be_bytes())
        .await
        .context("failed to write length")?;
    Ok(())
}

async fn read_message(
    stdout_reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut magic = [0u8; 6];
    stdout_reader
        .read_exact(&mut magic)
        .await
        .context("failed to read magic")?;
    let len = read_u32_be(stdout_reader).await? as usize;
    let mut payload = vec![0u8; len];
    stdout_reader
        .read_exact(&mut payload)
        .await
        .context("failed to read payload")?;
    Ok((magic.to_vec(), payload))
}

async fn send_code(
    stdin_writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    code: &str,
) -> Result<()> {
    let bytes = code.as_bytes();
    write_u32_be(stdin_writer, u32::try_from(bytes.len())?).await?;
    stdin_writer
        .write_all(bytes)
        .await
        .context("failed to write code")?;
    stdin_writer.flush().await.ok();
    Ok(())
}

async fn send_host_response(
    stdin_writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &Value,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).context("failed to serialize host response")?;
    write_u32_be(stdin_writer, u32::try_from(bytes.len())?).await?;
    stdin_writer
        .write_all(&bytes)
        .await
        .context("failed to write host response")?;
    stdin_writer.flush().await.ok();
    Ok(())
}

struct SandboxState {
    wasi: preview1::WasiP1Ctx,
}

/// Run JavaScript code inside the sandbox Wasm module.
///
/// This matches the protocol implemented in `ieapp-cli/src/ieapp/sandbox/runner.js`.
pub async fn run_script(
    wasm_path: &Path,
    code: &str,
    host_call_handler: Arc<HostCallHandler>,
    fuel_limit: u64,
) -> std::result::Result<Value, SandboxError> {
    if !wasm_path.exists() {
        return Err(SandboxError::MissingWasm(wasm_path.to_path_buf()));
    }

    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = wasmtime::Engine::new(&config).map_err(SandboxError::Other)?;
    let module = wasmtime::Module::from_file(&engine, wasm_path).map_err(SandboxError::Other)?;

    // stdin: host writes -> guest reads
    let (mut host_stdin, guest_stdin) = tokio::io::duplex(1024 * 1024);
    let guest_stdin_stream =
        AsyncStdinStream::new(wasmtime_wasi::pipe::AsyncReadStream::new(guest_stdin));

    // stdout: guest writes -> host reads
    let (guest_stdout, mut host_stdout) = tokio::io::duplex(1024 * 1024);
    let guest_stdout_stream = AsyncStdoutStream::new(wasmtime_wasi::pipe::AsyncWriteStream::new(
        1024 * 1024,
        guest_stdout,
    ));

    // stderr: guest writes -> host reads (ignored)
    let (guest_stderr, _host_stderr) = tokio::io::duplex(1024 * 1024);
    let guest_stderr_stream = AsyncStdoutStream::new(wasmtime_wasi::pipe::AsyncWriteStream::new(
        1024 * 1024,
        guest_stderr,
    ));

    let wasi = WasiCtxBuilder::new()
        .stdin(guest_stdin_stream)
        .stdout(guest_stdout_stream)
        .stderr(guest_stderr_stream)
        .build_p1();

    let mut linker: wasmtime::Linker<SandboxState> = wasmtime::Linker::new(&engine);
    preview1::add_to_linker_sync(&mut linker, |s| &mut s.wasi).map_err(SandboxError::Other)?;

    let guest = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut store = wasmtime::Store::new(&engine, SandboxState { wasi });
        store.set_fuel(fuel_limit).context("failed to set fuel")?;

        let instance = linker.instantiate(&mut store, &module)?;
        let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
        start.call(&mut store, ())?;
        Ok(())
    });

    // Send code and process multiplexed output.
    send_code(&mut host_stdin, code)
        .await
        .map_err(SandboxError::Other)?;

    let mut protocol_error: Option<anyhow::Error> = None;
    let result = loop {
        let (magic, payload) = match read_message(&mut host_stdout).await {
            Ok(v) => v,
            Err(e) => {
                // If the guest terminates unexpectedly (e.g. trap/fuel exhaustion),
                // stdout closes and we can't read further protocol frames.
                protocol_error = Some(e);
                break Value::Null;
            }
        };

        if magic == MAGIC_HOST {
            let call: Value =
                serde_json::from_slice(&payload).map_err(|e| SandboxError::Other(e.into()))?;
            let method = call
                .get("method")
                .and_then(|v| v.as_str())
                .ok_or_else(|| SandboxError::Execution("host.call missing method".to_string()))?;
            let path = call
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| SandboxError::Execution("host.call missing path".to_string()))?;
            let body = call.get("body").cloned();

            let resp = (host_call_handler)(method, path, body)
                .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}));
            send_host_response(&mut host_stdin, &resp)
                .await
                .map_err(SandboxError::Other)?;
            continue;
        }

        if magic == MAGIC_RESULT {
            let s = String::from_utf8(payload).map_err(|e| SandboxError::Other(e.into()))?;
            if s == "undefined" {
                break Value::Null;
            }
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| SandboxError::Execution(format!("invalid result json: {e}")))?;
            break v;
        }

        if magic == MAGIC_ERROR {
            let s = String::from_utf8(payload).map_err(|e| SandboxError::Other(e.into()))?;
            return Err(SandboxError::Execution(s));
        }

        return Err(SandboxError::Execution(format!(
            "invalid magic: {:?}",
            magic
        )));
    };

    match guest.await {
        Ok(Ok(())) => {
            if let Some(e) = protocol_error {
                Err(SandboxError::Other(e))
            } else {
                Ok(result)
            }
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            let msg_lower = msg.to_lowercase();
            // Normalize fuel exhaustion into a stable, user-facing error.
            // Wasmtime's exact wording can vary by version; we enforce that the
            // error string includes "fuel" for requirement traceability.
            if msg_lower.contains("fuel")
				|| msg_lower.contains("all fuel consumed")
				|| msg_lower.contains("out of fuel")
				// Some versions trap without mentioning fuel; the host side sees EOF.
				|| protocol_error
					.as_ref()
					.map(|pe| pe.to_string().to_lowercase().contains("failed to read magic"))
					.unwrap_or(false)
            {
                Err(SandboxError::Execution(format!("fuel exhausted: {msg}")))
            } else {
                Err(SandboxError::Other(e))
            }
        }
        Err(e) => Err(SandboxError::Other(e.into())),
    }
}
