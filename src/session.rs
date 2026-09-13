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
        "●"
    }

    pub fn color(self) -> &'static str {
        match self {
            State::Running => "#50fa7b",
            State::Waiting => "#ff79c6",
            State::Idle => "#ffffff",
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
    relative_time_at(epoch_ms, chrono::Utc::now().timestamp_millis())
}

fn relative_time_at(epoch_ms: i64, now_ms: i64) -> String {
    let secs = (now_ms - epoch_ms).max(0) / 1000;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(
        updated: i64,
        role: Option<&str>,
        completed: Option<i64>,
        finish: Option<&str>,
    ) -> Session {
        Session {
            id: "id".into(),
            title: "t".into(),
            directory: "/tmp".into(),
            updated,
            permission: None,
            role: role.map(String::from),
            completed,
            finish: finish.map(String::from),
            state: State::Idle,
        }
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    #[test]
    fn permission_means_waiting() {
        let mut s = session(now_ms(), None, None, None);
        s.permission = Some(serde_json::json!({"allow": true}));
        assert_eq!(derive_state(&s), State::Waiting);
    }

    #[test]
    fn user_turn_is_running() {
        assert_eq!(
            derive_state(&session(now_ms(), Some("user"), None, None)),
            State::Running
        );
    }

    #[test]
    fn unfinished_assistant_is_running() {
        assert_eq!(
            derive_state(&session(now_ms(), Some("assistant"), None, None)),
            State::Running
        );
    }

    #[test]
    fn recent_finished_assistant_is_waiting() {
        assert_eq!(
            derive_state(&session(now_ms(), Some("assistant"), Some(1), Some("stop"))),
            State::Waiting
        );
    }

    #[test]
    fn stale_finished_assistant_is_idle() {
        assert_eq!(
            derive_state(&session(
                now_ms() - 30 * 60_000,
                Some("assistant"),
                Some(1),
                Some("stop")
            )),
            State::Idle
        );
    }

    #[test]
    fn assistant_with_other_finish_is_idle() {
        assert_eq!(
            derive_state(&session(
                now_ms(),
                Some("assistant"),
                Some(1),
                Some("error")
            )),
            State::Idle
        );
    }

    #[test]
    fn empty_session_is_idle() {
        assert_eq!(
            derive_state(&session(now_ms(), None, None, None)),
            State::Idle
        );
    }

    #[test]
    fn relative_time_at_matches_window() {
        let now = 1_000_000_000_000;
        assert_eq!(relative_time_at(now, now), "now");
        assert_eq!(relative_time_at(now - 120_000, now), "2m");
        assert_eq!(relative_time_at(now - 7_200_000, now), "2h");
        assert_eq!(relative_time_at(now - 172_800_000, now), "2d");
        assert_eq!(relative_time_at(now + 60_000, now), "now");
    }

    #[test]
    fn short_dir_takes_basename() {
        assert_eq!(short_dir("/home/koen/projects/ocpop"), "ocpop");
        assert_eq!(short_dir("/home/koen/"), "koen");
        assert_eq!(short_dir(""), "");
    }

    #[test]
    fn truncate_leaves_short_strings() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_ellipsizes_long_strings() {
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("hello world", 1), "…");
        assert_eq!(truncate("hello world", 0), "");
    }

    #[test]
    fn pad_pads_with_nbsp() {
        assert_eq!(pad("ab", 4), "ab\u{a0}\u{a0}");
        assert_eq!(pad("ab", 2), "ab");
    }

    #[test]
    fn pad_truncates_when_over() {
        assert_eq!(pad("hello world", 3), "hel…");
    }

    #[test]
    fn state_glyph_is_constant() {
        for s in [State::Idle, State::Running, State::Waiting] {
            assert_eq!(s.glyph(), "●");
        }
    }

    #[test]
    fn state_colors_match_docs() {
        assert_eq!(State::Idle.color(), "#ffffff");
        assert_eq!(State::Running.color(), "#50fa7b");
        assert_eq!(State::Waiting.color(), "#ff79c6");
    }
}
