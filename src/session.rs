use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub directory: String,
    #[serde(rename = "time_updated")]
    pub updated: i64,
    pub permission: Option<serde_json::Value>,
    pub role: Option<String>,
    pub completed: Option<i64>,
    pub finish: Option<String>,
    #[serde(skip)]
    pub state: State,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum State {
    #[default]
    Idle,
    Running,
    Waiting,
}

impl State {
    /// same glyph for every state → columns stay aligned; color carries meaning
    pub fn glyph(self) -> &'static str {
        match self {
            State::Idle => "○",
            _ => "●",
        }
    }

    pub fn color(self) -> &'static str {
        match self {
            State::Running => "#50fa7b",
            State::Waiting => "#ff79c6",
            State::Idle => "#6272a4",
        }
    }
}

const RECENT_LIMIT: usize = 50;

fn db_json(query: &str) -> Result<Vec<serde_json::Value>> {
    let out = std::process::Command::new("opencode")
        .args(["db", query, "--format", "json"])
        .output()
        .context("running `opencode db`")?;
    if !out.status.success() {
        anyhow::bail!(
            "opencode db failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// epoch ms of the last boot, from /proc/stat btime (seconds)
fn boot_time_ms() -> i64 {
    std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|proc_stat| {
            proc_stat
                .lines()
                .find(|l| l.starts_with("btime"))
                .and_then(|line| line.split_whitespace().nth(1)?.parse::<i64>().ok())
        })
        .map_or(0, |secs| secs * 1000)
}

pub fn list() -> Result<Vec<Session>> {
    let rows = db_json(&format!(
        "SELECT s.id, s.title, s.directory, s.time_updated, \
         s.permission, \
         (SELECT json_extract(m.data,'$.role') FROM message m WHERE m.session_id = s.id ORDER BY m.time_created DESC LIMIT 1) AS role, \
         (SELECT json_extract(m.data,'$.time.completed') FROM message m WHERE m.session_id = s.id ORDER BY m.time_created DESC LIMIT 1) AS completed, \
         (SELECT json_extract(m.data,'$.finish') FROM message m WHERE m.session_id = s.id ORDER BY m.time_created DESC LIMIT 1) AS finish \
         FROM session s WHERE s.parent_id IS NULL \
         ORDER BY s.time_updated DESC LIMIT {RECENT_LIMIT}"
    ))?;

    let mut sessions: Vec<Session> = rows
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .context("parsing session rows")?;

    let boot_ms = boot_time_ms();
    for s in &mut sessions {
        s.state = derive_state(s);
    }
    sessions.retain(|s| s.updated >= boot_ms);
    Ok(sessions)
}

fn derive_state(s: &Session) -> State {
    if s.permission.is_some() {
        return State::Waiting;
    }
    match (s.role.as_deref(), s.completed, s.finish.as_deref()) {
        (Some("user"), _, _) => State::Running,
        (Some("assistant"), None, _) => State::Running,
        // turn just ended → waiting on user; stale finished sessions are idle
        (Some("assistant"), Some(_), Some("stop")) if is_recent(s.updated, 15 * 60_000) => {
            State::Waiting
        }
        _ => State::Idle,
    }
}

fn is_recent(epoch_ms: i64, window_ms: i64) -> bool {
    epoch_ms + window_ms > chrono::Utc::now().timestamp_millis()
}

pub fn relative_time(epoch_ms: i64) -> String {
    let secs = (chrono::Utc::now().timestamp_millis() - epoch_ms).max(0) / 1000;
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

pub fn short_dir(dir: &str) -> String {
    std::path::Path::new(dir)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string())
}

/// ellipsize to `width` display chars
pub fn truncate(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        return s.to_string();
    }
    let mut out: String = chars[..width.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}

/// pad right with non-breaking spaces (GTK collapses regular spaces)
pub fn pad(s: &str, width: usize) -> String {
    let mut out = truncate(s, width);
    let visible = out.chars().count();
    out.push_str(&"\u{a0}".repeat(width.saturating_sub(visible)));
    out
}
