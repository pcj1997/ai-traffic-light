use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

const HOOK_MARKER: &str = "codebuddy-light";
const LEGACY_EVENTS: &[&str] = &[
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

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codebuddy")
        .join("settings.json")
}

fn local_hook_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codebuddy-light")
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

fn command(path: &Path, state: &str, message: &str, extra: &str) -> String {
    let path = path.display().to_string();
    if cfg!(target_os = "windows") {
        let notification_only = if extra.is_empty() {
            ""
        } else {
            " -NotificationOnly"
        };
        format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File {} -State {} -Message {}{}",
            windows_quote(&path),
            windows_quote(state),
            windows_quote(message),
            notification_only
        )
    } else {
        format!(
            "python3 {} --state {} --message {} {}",
            shell_quote(&path),
            shell_quote(state),
            shell_quote(message),
            extra
        )
    }
}

fn hook(path: &Path, state: &str, message: &str, extra: &str) -> Value {
    json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": command(path, state, message, extra)
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
    entries.retain(|entry| !entry.to_string().contains(HOOK_MARKER));
    entries.push(definition);
}

fn remove_hook(hooks: &mut Map<String, Value>, event: &str) {
    let mut remove_event = false;
    if let Some(entries) = hooks.get_mut(event).and_then(Value::as_array_mut) {
        entries.retain(|entry| !entry.to_string().contains(HOOK_MARKER));
        remove_event = entries.is_empty();
    }
    if remove_event {
        hooks.remove(event);
    }
}

fn configure_hooks(hooks: &mut Map<String, Value>, destination: &Path) {
    for event in LEGACY_EVENTS {
        remove_hook(hooks, event);
    }
    append_hook(hooks, "SessionStart", hook(destination, "idle", "", ""));
    append_hook(
        hooks,
        "UserPromptSubmit",
        hook(destination, "working", "正在处理消息", ""),
    );
    append_hook(
        hooks,
        "PreToolUse",
        hook(destination, "working", "正在执行工具", ""),
    );
    append_hook(
        hooks,
        "PostToolUse",
        hook(destination, "working", "正在处理", ""),
    );
    append_hook(
        hooks,
        "PreCompact",
        hook(destination, "working", "正在压缩上下文", ""),
    );
    append_hook(
        hooks,
        "Stop",
        hook(destination, "completed", "回复完成", ""),
    );
    append_hook(hooks, "SessionEnd", hook(destination, "idle", "", ""));
}

fn contains_definition(hooks: &Map<String, Value>, event: &str, definition: &Value) -> bool {
    hooks
        .get(event)
        .and_then(Value::as_array)
        .is_some_and(|entries| entries.contains(definition))
}

fn configuration_matches(hooks: &Map<String, Value>, destination: &Path) -> bool {
    let mut expected = Map::new();
    configure_hooks(&mut expected, destination);

    expected.iter().all(|(event, definitions)| {
        definitions.as_array().is_some_and(|definitions| {
            definitions
                .iter()
                .all(|definition| contains_definition(hooks, event, definition))
        })
    }) && LEGACY_EVENTS.iter().all(|event| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .is_none_or(|entries| {
                entries
                    .iter()
                    .all(|entry| !entry.to_string().contains(HOOK_MARKER))
            })
    })
}

pub fn is_installed(_app: &AppHandle) -> bool {
    let destination = local_hook_path();
    let Ok(destination_content) = fs::read(&destination) else {
        return false;
    };
    if bundled_hook_content() != destination_content {
        return false;
    }

    let Ok(settings_content) = fs::read_to_string(settings_path()) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&settings_content) else {
        return false;
    };
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    configuration_matches(hooks, &destination)
}

pub fn install(_app: &AppHandle) -> Result<String, String> {
    let destination = local_hook_path();
    let parent = destination
        .parent()
        .ok_or_else(|| "无法解析 Hook 目标目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    fs::write(&destination, bundled_hook_content())
        .map_err(|error| format!("写入 Hook 失败（{}）：{}", destination.display(), error))?;

    let settings_path = settings_path();
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).map_err(|error| error.to_string())?;
        serde_json::from_str(&content)
            .map_err(|error| format!("现有 CodeBuddy 配置不是有效 JSON，未修改：{}", error))?
    } else {
        json!({})
    };

    let settings_object = settings
        .as_object_mut()
        .ok_or_else(|| "CodeBuddy 配置根节点必须是对象".to_string())?;
    let hooks = settings_object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "CodeBuddy hooks 配置必须是对象".to_string())?;

    configure_hooks(hooks, &destination);

    let content = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    fs::write(&settings_path, content).map_err(|error| error.to_string())?;
    Ok("Hooks 已安装".to_string())
}

#[cfg(test)]
mod tests {
    use super::{bundled_hook_content, configuration_matches, configure_hooks, hook, Map};
    use std::path::Path;

    #[test]
    fn configured_hooks_match_the_expected_installation() {
        let destination = Path::new("/tmp/codebuddy-light/status_writer.py");
        let mut hooks = Map::new();
        configure_hooks(&mut hooks, destination);

        assert!(configuration_matches(&hooks, destination));
    }

    #[test]
    fn bundled_hook_script_is_not_empty() {
        assert!(!bundled_hook_content().is_empty());
    }

    #[test]
    fn legacy_observer_requires_an_update() {
        let destination = Path::new("/tmp/codebuddy-light/status_writer.py");
        let mut hooks = Map::new();
        configure_hooks(&mut hooks, destination);
        hooks.insert(
            "Notification".to_string(),
            serde_json::json!([hook(destination, "waiting", "等待补充信息", "")]),
        );

        assert!(!configuration_matches(&hooks, destination));
    }
}
