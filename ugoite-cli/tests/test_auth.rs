//! CLI auth login tests.
//! REQ-OPS-015

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn spawn_recording_server(
    status_line: &'static str,
    body: &'static str,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut content_length = 0_usize;
                    let mut header_end: Option<usize> = None;

                    loop {
                        let mut buffer = [0_u8; 1024];
                        let read = match stream.read(&mut buffer) {
                            Ok(read) => read,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    || error.kind() == std::io::ErrorKind::TimedOut
                                    || error.kind() == std::io::ErrorKind::Interrupted =>
                            {
                                assert!(Instant::now() < deadline, "timed out waiting for request");
                                thread::sleep(Duration::from_millis(10));
                                continue;
                            }
                            Err(error) => panic!("failed to read request: {error}"),
                        };
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if header_end.is_none() {
                            if let Some(pos) =
                                request.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                let end = pos + 4;
                                header_end = Some(end);
                                let headers = String::from_utf8_lossy(&request[..end]);
                                for line in headers.lines() {
                                    let mut parts = line.splitn(2, ':');
                                    if let (Some(name), Some(value)) = (parts.next(), parts.next())
                                    {
                                        if name.eq_ignore_ascii_case("Content-Length") {
                                            content_length = value.trim().parse().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(end) = header_end {
                            if request.len() >= end + content_length {
                                break;
                            }
                        }
                    }

                    tx.send(String::from_utf8_lossy(&request).into_owned())
                        .unwrap();
                    let response = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept request: {error}"),
            }
        }
    });
    (format!("http://{}", addr), rx, handle)
}

#[test]
fn test_cli_auth_login_req_ops_015_posts_dev_login_and_prints_export() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"issued-token","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-auth-proxy-token: proxy-secret"));
    assert!(request_text
        .to_ascii_lowercase()
        .contains("x-ugoite-dev-passkey-context: passkey-context"));
    assert!(request_text.contains(r#""username":"dev-alice""#));
    assert!(request_text.contains(r#""totp_code":"123456""#));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("export UGOITE_AUTH_BEARER_TOKEN='issued-token'"));
}

#[test]
fn test_cli_auth_login_req_ops_015_shell_escapes_bearer_token_exports() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"unsafe' $(touch /tmp/ugoite-pwned) $HOME","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "export UGOITE_AUTH_BEARER_TOKEN='unsafe'\\'' $(touch /tmp/ugoite-pwned) $HOME'"
        ),
        "stdout was {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("POSIX shell quoting"),
        "stderr was {stderr}"
    );
}

#[test]
fn test_cli_auth_login_req_ops_015_quotes_empty_bearer_token_exports() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base, requests, handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"bearer_token":"","user_id":"dev-alice","expires_at":1900000000}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "backend", "--backend-url", &base])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args([
            "auth",
            "login",
            "--username",
            "dev-alice",
            "--totp-code",
            "123456",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env("UGOITE_DEV_AUTH_PROXY_TOKEN", "proxy-secret")
        .env("UGOITE_DEV_PASSKEY_CONTEXT", "passkey-context")
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let request_text = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.join().unwrap();

    assert!(request_text.starts_with("POST /auth/login HTTP/1.1"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("export UGOITE_AUTH_BEARER_TOKEN=''"),
        "stdout was {stdout}"
    );
}
