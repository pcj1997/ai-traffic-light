use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessesToUpdate, System};

const PROCESSING_STALE_SECS: u64 = 30 * 60;
const INACTIVE_STALE_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Idle = 0,
    Completed = 1,
    Working = 2,
    Waiting = 3,
    Error = 4,
}

impl State {
    fn from_str(value: &str) -> Self {
        match value {
            "completed" => Self::Completed,
            "working" => Self::Working,
            "waiting" => Self::Waiting,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Completed => "completed",
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "空闲",
            Self::Completed => "已完成",
            Self::Working => "处理中",
            Self::Waiting => "等待确认",
            Self::Error => "执行异常",
        }
    }
}

#[derive(Debug, Deserialize)]
struct SessionData {
    state: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    timestamp: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub id: String,
    pub title: String,
    pub state: String,
    pub label: String,
    pub message: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub state: String,
    pub label: String,
    pub message: String,
    pub session_count: usize,
    pub updated_at: u64,
    pub sessions: Vec<SessionSnapshot>,
}

impl StatusSnapshot {
    pub fn idle() -> Self {
        Self {
            state: State::Idle.key().to_string(),
            label: State::Idle.label().to_string(),
            message: String::new(),
            session_count: 0,
            updated_at: 0,
            sessions: Vec::new(),
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codebuddy-light")
        .join("sessions")
}

fn is_expired(state: State, age: u64) -> bool {
    match state {
        State::Idle => true,
        State::Working | State::Error => age > PROCESSING_STALE_SECS,
        State::Completed | State::Waiting => age > INACTIVE_STALE_SECS,
    }
}

fn is_codebuddy_process(name: &str, executable: &str) -> bool {
    let identity = format!("{name} {executable}").to_lowercase();
    identity.contains("codebuddy")
        && !identity.contains("codebuddy-light")
        && !identity.contains("codebuddy light")
}

fn codebuddy_is_running(system: &System) -> bool {
    system.processes().values().any(|process| {
        is_codebuddy_process(
            &process.name().to_string_lossy(),
            &process
                .exe()
                .map(|path| path.to_string_lossy())
                .unwrap_or_default(),
        )
    })
}

fn session_paths() -> Vec<PathBuf> {
    fs::read_dir(sessions_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect()
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

pub fn delete_session(id: &str) -> Result<(), String> {
    if !valid_session_id(id) {
        return Err("无效的会话 ID".to_string());
    }
    fs::remove_file(sessions_dir().join(format!("{id}.json")))
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("删除会话失败：{error}"))
}

pub fn clear_sessions() -> usize {
    session_paths()
        .into_iter()
        .filter(|path| fs::remove_file(path).is_ok())
        .count()
}

pub struct SessionLifecycleMonitor {
    codebuddy_was_running: bool,
    observed_sessions: HashSet<PathBuf>,
    system: System,
}

impl Default for SessionLifecycleMonitor {
    fn default() -> Self {
        Self {
            codebuddy_was_running: false,
            observed_sessions: HashSet::new(),
            system: System::new_all(),
        }
    }
}

impl SessionLifecycleMonitor {
    pub fn poll(&mut self) {
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        let codebuddy_is_running = codebuddy_is_running(&self.system);
        if codebuddy_is_running {
            self.observed_sessions.extend(session_paths());
        } else if self.codebuddy_was_running {
            for path in self.observed_sessions.drain() {
                let _ = fs::remove_file(path);
            }
        }
        self.codebuddy_was_running = codebuddy_is_running;
    }
}

fn session_title(id: &str, cwd: &str) -> String {
    PathBuf::from(cwd)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("会话 {}", id.chars().take(8).collect::<String>()))
}

pub fn read_status() -> StatusSnapshot {
    let dir = sessions_dir();
    let _ = fs::create_dir_all(&dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return StatusSnapshot::idle();
    };

    let now = now_secs();
    let mut best = State::Idle;
    let mut best_message = String::new();
    let mut best_timestamp = 0;
    let mut session_count = 0;
    let mut sessions = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<SessionData>(&content) else {
            continue;
        };
        let age = now.saturating_sub(data.timestamp);
        let state = State::from_str(&data.state);
        if is_expired(state, age) {
            let _ = fs::remove_file(path);
            continue;
        }

        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        session_count += 1;
        sessions.push(SessionSnapshot {
            title: session_title(&id, &data.cwd),
            id,
            state: state.key().to_string(),
            label: state.label().to_string(),
            message: data.message.clone(),
            updated_at: data.timestamp,
        });
        if state > best || (state == best && data.timestamp > best_timestamp) {
            best = state;
            best_message = data.message;
            best_timestamp = data.timestamp;
        }
    }

    sessions.sort_by(|left, right| {
        let left_state = State::from_str(&left.state);
        let right_state = State::from_str(&right.state);
        right_state
            .cmp(&left_state)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });

    StatusSnapshot {
        state: best.key().to_string(),
        label: best.label().to_string(),
        message: best_message,
        session_count,
        updated_at: best_timestamp,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_codebuddy_process, is_expired, session_title, valid_session_id, State,
        INACTIVE_STALE_SECS, PROCESSING_STALE_SECS,
    };

    #[test]
    fn completed_and_waiting_states_have_a_long_safety_timeout() {
        assert!(!is_expired(State::Completed, INACTIVE_STALE_SECS));
        assert!(!is_expired(State::Waiting, INACTIVE_STALE_SECS));
        assert!(is_expired(State::Completed, INACTIVE_STALE_SECS + 1));
        assert!(is_expired(State::Waiting, INACTIVE_STALE_SECS + 1));
    }

    #[test]
    fn processing_states_have_a_safety_timeout() {
        assert!(!is_expired(State::Working, PROCESSING_STALE_SECS));
        assert!(is_expired(State::Working, PROCESSING_STALE_SECS + 1));
        assert!(is_expired(State::Error, PROCESSING_STALE_SECS + 1));
    }

    #[test]
    fn session_titles_prefer_the_project_directory() {
        assert_eq!(
            session_title("1234567890", "/tmp/codebuddy-light"),
            "codebuddy-light"
        );
        assert_eq!(session_title("1234567890", ""), "会话 12345678");
    }

    #[test]
    fn codebuddy_process_detection_excludes_the_status_light() {
        assert!(is_codebuddy_process(
            "Electron",
            "/Applications/CodeBuddy CN Enterprise.app/Contents/MacOS/Electron"
        ));
        assert!(is_codebuddy_process(
            "CodeBuddy CN.exe",
            r"C:\Program Files\CodeBuddy CN\CodeBuddy CN.exe"
        ));
        assert!(!is_codebuddy_process(
            "codebuddy-light",
            "/Applications/CodeBuddy Light.app/Contents/MacOS/codebuddy-light"
        ));
    }

    #[test]
    fn session_ids_cannot_escape_the_sessions_directory() {
        assert!(valid_session_id("conversation-123_abc"));
        assert!(!valid_session_id(""));
        assert!(!valid_session_id(".."));
        assert!(!valid_session_id("../settings"));
        assert!(!valid_session_id("session/path"));
    }
}
