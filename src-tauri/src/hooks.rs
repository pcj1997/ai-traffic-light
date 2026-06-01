use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const HOOK_MARKERS: &[&str] = &["ai-traffic-light", "codebuddy-light"];
const CODEBUDDY_LEGACY_EVENTS: &[&str] = &[
    "PermissionRequest",
    "Notification",
    "Elicitation",
    "ElicitationResult",
    "SubagentStart",
    "SubagentStop",
    "TaskCompleted",
    "PostToolUseFailure",
    "StopFailure",
];

#[derive(Default)]
struct CommandOptions {
    notification_only: bool,
    emit_empty_json: bool,
}

fn codebuddy_settings_path() -> PathBuf {
    home_dir().join(".codebuddy").join("settings.json")
}

fn claude_settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

fn codex_hooks_path() -> PathBuf {
    home_dir().join(".codex").join("hooks.json")
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn local_hook_path() -> PathBuf {
    home_dir()
        .join(".ai-traffic-light")
        .join("hooks")
        .join(hook_script_name())
}

fn bundled_hook_content() -> &'static [u8] {
    if cfg!(target_os = "windows") {
        include_bytes!("../../hooks/status_writer.ps1")
    } else {
        include_bytes!("../../hooks/status_writer.py")
    }
}

fn hook_script_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "status_writer.ps1"
    } else {
        "status_writer.py"
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn windows_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn command(
    path: &Path,
    client: &str,
    state: &str,
    message: &str,
    options: &CommandOptions,
) -> String {
    let path = path.display().to_string();
    if cfg!(target_os = "windows") {
        let notification_only = if options.notification_only {
            " -NotificationOnly"
        } else {
            ""
        };
        let emit_empty_json = if options.emit_empty_json {
            " -EmitEmptyJson"
        } else {
            ""
        };
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File {} -Client {} -State {} -Message {}{}{}",
            windows_quote(&path),
            windows_quote(client),
            windows_quote(state),
            windows_quote(message),
            notification_only,
            emit_empty_json
        )
    } else {
        let notification_only = if options.notification_only {
            " --notification-only"
        } else {
            ""
        };
        let emit_empty_json = if options.emit_empty_json {
            " --emit-empty-json"
        } else {
            ""
        };
        format!(
            "python3 {} --client {} --state {} --message {}{}{}",
            shell_quote(&path),
            shell_quote(client),
            shell_quote(state),
            shell_quote(message),
            notification_only,
            emit_empty_json
        )
    }
}

fn hook(path: &Path, client: &str, state: &str, message: &str) -> Value {
    hook_with_options(path, client, state, message, CommandOptions::default())
}

fn hook_with_options(
    path: &Path,
    client: &str,
    state: &str,
    message: &str,
    options: CommandOptions,
) -> Value {
    json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": command(path, client, state, message, &options)
        }]
    })
}

fn append_hook(hooks: &mut Map<String, Value>, event: &str, definition: Value) {
    let entries = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(entries) = entries.as_array_mut() else {
        return;
    };
    entries.retain(|entry| !is_managed_hook(entry));
    entries.push(definition);
}

fn is_managed_hook(entry: &Value) -> bool {
    let content = entry.to_string();
    HOOK_MARKERS.iter().any(|marker| content.contains(marker))
}

fn remove_hook(hooks: &mut Map<String, Value>, event: &str) {
    let mut remove_event = false;
    if let Some(entries) = hooks.get_mut(event).and_then(Value::as_array_mut) {
        entries.retain(|entry| !is_managed_hook(entry));
        remove_event = entries.is_empty();
    }
    if remove_event {
        hooks.remove(event);
    }
}

fn configure_codebuddy_hooks(hooks: &mut Map<String, Value>, destination: &Path) {
    for event in CODEBUDDY_LEGACY_EVENTS {
        remove_hook(hooks, event);
    }
    append_hook(
        hooks,
        "SessionStart",
        hook(destination, "codebuddy", "idle", ""),
    );
    append_hook(
        hooks,
        "UserPromptSubmit",
        hook(destination, "codebuddy", "working", "正在处理消息"),
    );
    append_hook(
        hooks,
        "PreToolUse",
        hook(destination, "codebuddy", "working", "正在执行工具"),
    );
    append_hook(
        hooks,
        "PostToolUse",
        hook(destination, "codebuddy", "working", "正在处理"),
    );
    append_hook(
        hooks,
        "PreCompact",
        hook(destination, "codebuddy", "working", "正在压缩上下文"),
    );
    append_hook(
        hooks,
        "Stop",
        hook(destination, "codebuddy", "completed", "回复完成"),
    );
    append_hook(
        hooks,
        "SessionEnd",
        hook(destination, "codebuddy", "idle", ""),
    );
}

fn configure_claude_hooks(hooks: &mut Map<String, Value>, destination: &Path) {
    append_hook(
        hooks,
        "SessionStart",
        hook(destination, "claude", "idle", ""),
    );
    append_hook(
        hooks,
        "UserPromptSubmit",
        hook(destination, "claude", "working", "正在处理消息"),
    );
    append_hook(
        hooks,
        "PreToolUse",
        hook(destination, "claude", "working", "正在执行工具"),
    );
    append_hook(
        hooks,
        "PermissionRequest",
        hook(destination, "claude", "waiting", "等待权限确认"),
    );
    append_hook(
        hooks,
        "PostToolUse",
        hook(destination, "claude", "working", "正在处理"),
    );
    append_hook(
        hooks,
        "PostToolUseFailure",
        hook(destination, "claude", "error", "工具执行失败"),
    );
    append_hook(
        hooks,
        "Notification",
        hook_with_options(
            destination,
            "claude",
            "working",
            "",
            CommandOptions {
                notification_only: true,
                ..CommandOptions::default()
            },
        ),
    );
    append_hook(
        hooks,
        "PreCompact",
        hook(destination, "claude", "working", "正在压缩上下文"),
    );
    append_hook(
        hooks,
        "Stop",
        hook(destination, "claude", "completed", "回复完成"),
    );
    append_hook(
        hooks,
        "StopFailure",
        hook(destination, "claude", "error", "响应失败"),
    );
    append_hook(hooks, "SessionEnd", hook(destination, "claude", "idle", ""));
}

fn configure_codex_hooks(hooks: &mut Map<String, Value>, destination: &Path) {
    append_hook(
        hooks,
        "SessionStart",
        hook(destination, "codex", "idle", ""),
    );
    append_hook(
        hooks,
        "UserPromptSubmit",
        hook(destination, "codex", "working", "正在处理消息"),
    );
    append_hook(
        hooks,
        "PreToolUse",
        hook(destination, "codex", "working", "正在执行工具"),
    );
    append_hook(
        hooks,
        "PermissionRequest",
        hook(destination, "codex", "waiting", "等待权限确认"),
    );
    append_hook(
        hooks,
        "PostToolUse",
        hook(destination, "codex", "working", "正在处理"),
    );
    append_hook(
        hooks,
        "PreCompact",
        hook(destination, "codex", "working", "正在压缩上下文"),
    );
    append_hook(
        hooks,
        "PostCompact",
        hook(destination, "codex", "working", "正在处理"),
    );
    append_hook(
        hooks,
        "Stop",
        hook_with_options(
            destination,
            "codex",
            "completed",
            "回复完成",
            CommandOptions {
                emit_empty_json: true,
                ..CommandOptions::default()
            },
        ),
    );
}

fn contains_definition(hooks: &Map<String, Value>, event: &str, definition: &Value) -> bool {
    hooks
        .get(event)
        .and_then(Value::as_array)
        .is_some_and(|entries| entries.contains(definition))
}

fn configuration_matches(
    hooks: &Map<String, Value>,
    destination: &Path,
    configure: fn(&mut Map<String, Value>, &Path),
) -> bool {
    let mut expected = Map::new();
    configure(&mut expected, destination);
    expected.iter().all(|(event, definitions)| {
        definitions.as_array().is_some_and(|definitions| {
            definitions
                .iter()
                .all(|definition| contains_definition(hooks, event, definition))
        })
    })
}

fn codebuddy_configuration_matches(hooks: &Map<String, Value>, destination: &Path) -> bool {
    configuration_matches(hooks, destination, configure_codebuddy_hooks)
        && CODEBUDDY_LEGACY_EVENTS.iter().all(|event| {
            hooks
                .get(*event)
                .and_then(Value::as_array)
                .is_none_or(|entries| entries.iter().all(|entry| !is_managed_hook(entry)))
        })
}

fn json_hooks_match(
    path: PathBuf,
    destination: &Path,
    configure: fn(&mut Map<String, Value>, &Path),
) -> bool {
    let Ok(settings_content) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&settings_content) else {
        return false;
    };
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    configuration_matches(hooks, destination, configure)
}

pub fn is_installed(_app: &AppHandle) -> bool {
    let destination = local_hook_path();
    let Ok(destination_content) = fs::read(&destination) else {
        return false;
    };
    if bundled_hook_content() != destination_content {
        return false;
    }

    let Ok(settings_content) = fs::read_to_string(codebuddy_settings_path()) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&settings_content) else {
        return false;
    };
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };

    codebuddy_configuration_matches(hooks, &destination)
        && json_hooks_match(claude_settings_path(), &destination, configure_claude_hooks)
        && json_hooks_match(codex_hooks_path(), &destination, configure_codex_hooks)
}

fn install_json_hooks(
    settings_path: PathBuf,
    destination: &Path,
    label: &str,
    configure: fn(&mut Map<String, Value>, &Path),
) -> Result<(), String> {
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).map_err(|error| error.to_string())?;
        serde_json::from_str(&content)
            .map_err(|error| format!("现有 {label} 配置不是有效 JSON，未修改：{error}"))?
    } else {
        json!({})
    };
    let settings_object = settings
        .as_object_mut()
        .ok_or_else(|| format!("{label} 配置根节点必须是对象"))?;
    let hooks = settings_object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{label} hooks 配置必须是对象"))?;
    configure(hooks, destination);
    let content = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    fs::write(settings_path, content).map_err(|error| error.to_string())
}

pub fn install(_app: &AppHandle) -> Result<String, String> {
    let destination = local_hook_path();
    let parent = destination
        .parent()
        .ok_or_else(|| "无法解析 Hook 目标目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    fs::write(&destination, bundled_hook_content())
        .map_err(|error| format!("写入 Hook 失败（{}）：{}", destination.display(), error))?;

    install_json_hooks(
        codebuddy_settings_path(),
        &destination,
        "CodeBuddy",
        configure_codebuddy_hooks,
    )?;
    install_json_hooks(
        claude_settings_path(),
        &destination,
        "Claude Code",
        configure_claude_hooks,
    )?;
    install_json_hooks(
        codex_hooks_path(),
        &destination,
        "Codex",
        configure_codex_hooks,
    )?;
    Ok("CodeBuddy、Codex 和 Claude Hooks 已安装".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        bundled_hook_content, codebuddy_configuration_matches, configuration_matches,
        configure_claude_hooks, configure_codebuddy_hooks, configure_codex_hooks, hook,
        install_json_hooks, Map,
    };
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn configured_hooks_match_the_expected_installation() {
        let destination = Path::new("/tmp/ai-traffic-light/status_writer.py");
        for configure in [
            configure_codebuddy_hooks as fn(&mut Map<String, serde_json::Value>, &Path),
            configure_claude_hooks,
            configure_codex_hooks,
        ] {
            let mut hooks = Map::new();
            configure(&mut hooks, destination);
            assert!(configuration_matches(&hooks, destination, configure));
        }
    }

    #[test]
    fn bundled_hook_script_is_not_empty() {
        assert!(!bundled_hook_content().is_empty());
    }

    #[test]
    fn legacy_codebuddy_observer_requires_an_update() {
        let destination = Path::new("/tmp/ai-traffic-light/status_writer.py");
        let mut hooks = Map::new();
        configure_codebuddy_hooks(&mut hooks, destination);
        hooks.insert(
            "Notification".to_string(),
            serde_json::json!([hook(destination, "codebuddy", "waiting", "等待补充信息")]),
        );
        assert!(!codebuddy_configuration_matches(&hooks, destination));
    }

    #[test]
    fn installing_hooks_preserves_unrelated_settings() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let settings_path =
            std::env::temp_dir().join(format!("ai-traffic-light-settings-{unique}.json"));
        fs::write(
            &settings_path,
            r#"{"env":{"EXISTING":"preserved"},"permissions":{"allow":["Read"]}}"#,
        )
        .unwrap();

        let destination = Path::new("/tmp/ai-traffic-light/status_writer.py");
        install_json_hooks(
            settings_path.clone(),
            destination,
            "Claude Code",
            configure_claude_hooks,
        )
        .unwrap();

        let settings: Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(settings["env"]["EXISTING"], "preserved");
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        assert!(settings["hooks"]["PermissionRequest"].is_array());
        let _ = fs::remove_file(settings_path);
    }

    #[test]
    fn installing_new_hooks_removes_legacy_hook_commands() {
        let destination = Path::new("/tmp/ai-traffic-light/status_writer.py");
        let legacy_destination = Path::new("/tmp/codebuddy-light/status_writer.py");
        let mut hooks = Map::new();
        hooks.insert(
            "SessionStart".to_string(),
            serde_json::json!([hook(legacy_destination, "codex", "idle", "")]),
        );

        configure_codex_hooks(&mut hooks, destination);

        let entries = hooks["SessionStart"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].to_string().contains("ai-traffic-light"));
    }
}
