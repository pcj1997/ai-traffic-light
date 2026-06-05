use crate::sessions::{write_session_update, SessionUpdate};
use serde::Serialize;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

const DEFAULT_PORT: u16 = 37628;
const MAX_BODY_SIZE: usize = 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

static BRIDGE: OnceLock<BridgeStatus> = OnceLock::new();
static BRIDGE_ERROR: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    pub local_url: String,
    pub remote_url: String,
    pub ssh_reverse_tunnel_command: String,
    pub remote_installer_path: String,
    pub remote_install_command: String,
    pub error_message: String,
}

fn bridge_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".ai-traffic-light")
}

fn token_path() -> PathBuf {
    bridge_dir().join("bridge-token")
}

fn remote_installer_path() -> PathBuf {
    bridge_dir().join("install-codebuddy-remote-hook.sh")
}

fn generate_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("生成桥接 token 失败，系统随机源不可用：{error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn write_token(path: PathBuf, token: &str) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    file.write_all(token.as_bytes())
        .map_err(|error| error.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn bridge_token() -> Result<String, String> {
    let path = token_path();
    if let Ok(token) = fs::read_to_string(&path) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = fs::Permissions::from_mode(0o600);
                let _ = fs::set_permissions(&path, permissions);
            }
            return Ok(token);
        }
    }

    let token = generate_token()?;
    fs::create_dir_all(bridge_dir()).map_err(|error| error.to_string())?;
    write_token(path, &token)?;
    Ok(token)
}

fn build_status(port: u16, token: String) -> BridgeStatus {
    let local_url = format!("http://127.0.0.1:{port}/hook/{token}");
    let remote_url = local_url.clone();
    BridgeStatus {
        enabled: true,
        port,
        token,
        local_url,
        remote_url,
        ssh_reverse_tunnel_command: format!("ssh -N -R {port}:127.0.0.1:{port} <user>@<server>"),
        remote_installer_path: remote_installer_path().display().to_string(),
        remote_install_command: "bash install-codebuddy-remote-hook.sh".to_string(),
        error_message: String::new(),
    }
}

fn bind_listener() -> Result<TcpListener, String> {
    TcpListener::bind(("127.0.0.1", DEFAULT_PORT))
        .map_err(|error| format!("启动远程桥接监听失败：{error}"))
}

pub fn start() -> Result<BridgeStatus, String> {
    if let Some(status) = BRIDGE.get() {
        return Ok(status.clone());
    }

    let token = bridge_token().inspect_err(|error| {
        let _ = BRIDGE_ERROR.set(error.clone());
    })?;
    let listener = bind_listener().inspect_err(|error| {
        let _ = BRIDGE_ERROR.set(error.clone());
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let status = build_status(port, token.clone());
    let _ = BRIDGE.set(status.clone());

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let token = token.clone();
            thread::spawn(move || {
                let _ = handle_connection(stream, &token);
            });
        }
    });

    Ok(status)
}

pub fn status() -> BridgeStatus {
    BRIDGE.get().cloned().unwrap_or_else(|| BridgeStatus {
        enabled: false,
        port: 0,
        token: String::new(),
        local_url: String::new(),
        remote_url: String::new(),
        ssh_reverse_tunnel_command: String::new(),
        remote_installer_path: remote_installer_path().display().to_string(),
        remote_install_command: String::new(),
        error_message: BRIDGE_ERROR.get().cloned().unwrap_or_default(),
    })
}

fn response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

fn read_http_request(stream: &mut TcpStream) -> Result<(String, String), String> {
    let mut buffer = Vec::new();
    let mut temporary = [0_u8; 2048];
    let header_end = loop {
        let count = stream
            .read(&mut temporary)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("empty request".to_string());
        }
        buffer.extend_from_slice(&temporary[..count]);
        if buffer.len() > MAX_BODY_SIZE {
            return Err("request too large".to_string());
        }
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = parse_content_length(&headers).unwrap_or(0);
    if content_length > MAX_BODY_SIZE {
        return Err("request body too large".to_string());
    }

    while buffer.len() < header_end + content_length {
        let count = stream
            .read(&mut temporary)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&temporary[..count]);
    }
    if buffer.len() < header_end + content_length {
        return Err("request body shorter than content-length".to_string());
    }

    let body =
        String::from_utf8_lossy(&buffer[header_end..header_end + content_length]).to_string();
    Ok((headers, body))
}

fn handle_connection(mut stream: TcpStream, token: &str) -> Result<(), String> {
    stream
        .set_read_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|error| error.to_string())?;

    let (headers, body) = read_http_request(&mut stream)?;
    let request_line = headers.lines().next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let path = request_parts.next().unwrap_or_default();
    let expected_path = format!("/hook/{token}");

    if method == "GET" && path == format!("/health/{token}") {
        return response(&mut stream, "200 OK", r#"{"ok":true}"#)
            .map_err(|error| error.to_string());
    }

    if method != "POST" || path != expected_path {
        return response(&mut stream, "404 Not Found", r#"{"ok":false}"#)
            .map_err(|error| error.to_string());
    }

    let update = serde_json::from_str::<SessionUpdate>(&body).map_err(|error| error.to_string())?;
    write_session_update(update)?;
    response(&mut stream, "200 OK", r#"{"ok":true}"#).map_err(|error| error.to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
fn codebuddy_hook_command(hook_path: &str, bridge_url: &str, state: &str, message: &str) -> String {
    format!(
        "python3 {} --client codebuddy --state {} --message {} --bridge-url {}",
        shell_quote(hook_path),
        shell_quote(state),
        shell_quote(message),
        shell_quote(bridge_url)
    )
}

pub fn write_remote_installer() -> Result<BridgeStatus, String> {
    let status = start()?;
    let bridge_url = &status.remote_url;
    let hook_content = include_str!("../../hooks/status_writer.py");
    let events = [
        ("SessionStart", "idle", ""),
        ("UserPromptSubmit", "working", "正在处理消息"),
        ("PreToolUse", "working", "正在执行工具"),
        ("PostToolUse", "working", "正在处理"),
        ("PreCompact", "working", "正在压缩上下文"),
        ("Stop", "completed", "回复完成"),
        ("SessionEnd", "idle", ""),
    ];
    let hooks_json = serde_json::to_string(&events).map_err(|error| error.to_string())?;
    let installer = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

mkdir -p "$HOME/.ai-traffic-light/hooks" "$HOME/.codebuddy"
cat > "$HOME/.ai-traffic-light/hooks/status_writer.py" <<'AI_TRAFFIC_LIGHT_STATUS_WRITER'
{hook_content}
AI_TRAFFIC_LIGHT_STATUS_WRITER
chmod +x "$HOME/.ai-traffic-light/hooks/status_writer.py"

AI_TRAFFIC_LIGHT_REMOTE_HOOKS={hooks_json} \
AI_TRAFFIC_LIGHT_BRIDGE_URL={bridge_url} \
python3 - <<'PY'
import json
import os
import shlex
import shutil
import time
from pathlib import Path

settings_path = Path.home() / ".codebuddy" / "settings.json"
backup_path = None
if settings_path.exists():
    backup_path = settings_path.with_name(f"settings.ai-traffic-light-backup-{{int(time.time())}}.json")
    shutil.copy2(settings_path, backup_path)

try:
    raw_settings = settings_path.read_text(encoding="utf-8-sig").strip()
except FileNotFoundError:
    raw_settings = ""

if raw_settings:
    try:
        settings = json.loads(raw_settings)
    except json.JSONDecodeError:
        print(f"Existing CodeBuddy settings is invalid JSON; backup saved to {{backup_path}}. Rebuilding hooks config.")
        settings = {{}}
else:
    settings = {{}}

if not isinstance(settings, dict):
    print(f"Existing CodeBuddy settings root is not an object; backup saved to {{backup_path}}. Rebuilding hooks config.")
    settings = {{}}

hooks = settings.get("hooks")
if not isinstance(hooks, dict):
    if hooks is not None:
        print(f"Existing CodeBuddy hooks config is not an object; backup saved to {{backup_path}}. Replacing hooks config.")
    hooks = {{}}
    settings["hooks"] = hooks

hook_path = str(Path.home() / ".ai-traffic-light" / "hooks" / "status_writer.py")
bridge_url = os.environ["AI_TRAFFIC_LIGHT_BRIDGE_URL"]
events = json.loads(os.environ["AI_TRAFFIC_LIGHT_REMOTE_HOOKS"])

def managed(entry):
    return "ai-traffic-light" in json.dumps(entry, ensure_ascii=False) or "codebuddy-light" in json.dumps(entry, ensure_ascii=False)

for event, state, message in events:
    command = {command_builder}
    entries = hooks.get(event)
    if not isinstance(entries, list):
        if entries is not None:
            print(f"Existing CodeBuddy hook event {{event}} is not a list; backup saved to {{backup_path}}. Replacing this event.")
        entries = []
        hooks[event] = entries
    entries[:] = [entry for entry in entries if not managed(entry)]
    entries.append({{"matcher": "", "hooks": [{{"type": "command", "command": command}}]}})

settings_path.write_text(json.dumps(settings, ensure_ascii=False, indent=2), encoding="utf-8")
print(f"AI Traffic Light remote CodeBuddy hooks installed: {{settings_path}}")
if backup_path:
    print(f"Backup saved: {{backup_path}}")
PY
"#,
        hook_content = hook_content,
        hooks_json = shell_quote(&hooks_json),
        bridge_url = shell_quote(bridge_url),
        command_builder = r#"" ".join([
        "python3",
        shlex.quote(hook_path),
        "--client",
        "codebuddy",
        "--state",
        shlex.quote(state),
        "--message",
        shlex.quote(message),
        "--bridge-url",
        shlex.quote(bridge_url),
    ])"#
    );

    let path = remote_installer_path();
    fs::create_dir_all(bridge_dir()).map_err(|error| error.to_string())?;
    fs::write(&path, installer).map_err(|error| error.to_string())?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::{build_status, codebuddy_hook_command, parse_content_length};

    #[test]
    fn bridge_status_uses_loopback_urls() {
        let status = build_status(37628, "secret".to_string());
        assert_eq!(status.local_url, "http://127.0.0.1:37628/hook/secret");
        assert!(status
            .ssh_reverse_tunnel_command
            .contains("-R 37628:127.0.0.1:37628"));
    }

    #[test]
    fn http_content_length_is_parsed_case_insensitively() {
        assert_eq!(
            parse_content_length("POST / HTTP/1.1\r\ncontent-length: 42"),
            Some(42)
        );
    }

    #[test]
    fn remote_codebuddy_command_enables_bridge_mode() {
        let command = codebuddy_hook_command(
            "$HOME/.ai-traffic-light/hooks/status_writer.py",
            "http://127.0.0.1:37628/hook/token",
            "working",
            "正在处理",
        );
        assert!(command.contains("--bridge-url"));
        assert!(command.contains("--client codebuddy"));
    }
}
