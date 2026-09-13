use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Deserialize;

const CACHE: &str = "/tmp/ocpop-usage.json";
const REFRESH_SECS: u64 = 60;

#[derive(Deserialize)]
struct UsageResp {
    usage: Usage,
}

#[derive(Deserialize)]
struct Usage {
    monthly: Window,
    weekly: Window,
    rolling: Window,
}

#[derive(Deserialize)]
struct Window {
    percent: Option<f64>,
    #[serde(rename = "resetsAt")]
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct Auth {
    #[serde(rename = "opencode-go")]
    go: AuthEntry,
}

#[derive(Deserialize)]
struct AuthEntry {
    key: String,
}

pub struct UsageStatus {
    pub tooltip: Option<String>,
    pub error: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache() -> Option<(u64, String)> {
    let raw = std::fs::read_to_string(CACHE).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some((
        json.get("fetched_at")?.as_u64()?,
        json.get("body")?.as_str()?.to_string(),
    ))
}

fn write_cache(body: &str) -> Result<()> {
    let json = serde_json::json!({ "fetched_at": now_secs(), "body": body });
    std::fs::write(CACHE, json.to_string())?;
    Ok(())
}

fn fetch() -> Option<String> {
    let raw = std::fs::read_to_string(format!(
        "{}/.local/share/opencode/auth.json",
        std::env::var("HOME").ok()?
    ))
    .ok()?;
    let key = serde_json::from_str::<Auth>(&raw).ok()?.go.key;

    let out = std::process::Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "10",
            "-H",
            &format!("Authorization: Bearer {key}"),
            "https://opencode.ai/zen/go/v1/usage",
        ])
        .output()
        .ok()?;
    let body = String::from_utf8_lossy(&out.stdout).into_owned();
    if body.is_empty() {
        return None;
    }
    serde_json::from_str::<UsageResp>(&body).ok()?;
    write_cache(&body).ok()?;
    Some(body)
}

fn parse(body: &str) -> Option<UsageResp> {
    serde_json::from_str(body).ok()
}

pub fn status() -> UsageStatus {
    let now = now_secs();
    let cached = read_cache();

    let body = if cached
        .as_ref()
        .is_some_and(|(fetched_at, _)| now.saturating_sub(*fetched_at) < REFRESH_SECS)
    {
        cached.map(|(_, b)| b)
    } else {
        fetch().or_else(|| cached.map(|(_, b)| b))
    };

    let Some(body) = body.filter(|b| parse(b).is_some()) else {
        return UsageStatus {
            tooltip: None,
            error: Some("usage unavailable".into()),
        };
    };

    let resp: UsageResp = serde_json::from_str(&body).unwrap();
    UsageStatus {
        tooltip: Some(render(&resp.usage)),
        error: None,
    }
}

fn render(u: &Usage) -> String {
    format!(
        "<span size=\"large\"><b>OpenCode Go</b></span>\n{}\n{}\n{}",
        fmt_window("5h", &u.rolling, 18_000.0),
        fmt_window("7d", &u.weekly, 604_800.0),
        fmt_window("30d", &u.monthly, 2_592_000.0),
    )
}

fn fmt_window(label: &str, win: &Window, dur_secs: f64) -> String {
    let Some(pct) = (win.percent).map(|p| p.clamp(0.0, 100.0)) else {
        return format!("<span color=\"#ff5555\"><b>{label}</b></span> <i>n/a</i>");
    };
    let remaining = win
        .resets_at
        .as_deref()
        .and_then(|iso| chrono::DateTime::parse_from_rfc3339(iso).ok())
        .map(|dt| (dt.timestamp() - chrono::Utc::now().timestamp()).max(0) as f64)
        .unwrap_or(0.0);

    // fixed columns → aligned in a <tt> block
    let padded_label = format!("{label:<3}");
    format!(
        "<b>{padded_label}</b> <span color=\"{color}\">{pct:>4}% {bar} {left:>7}</span>",
        color = bar_color(pct, remaining, dur_secs),
        bar = braille_bar::BrailleBar::new(10).render(pct),
        left = remaining_str(remaining),
    )
}

fn bar_color(pct: f64, remaining: f64, dur: f64) -> &'static str {
    let elapsed_pct = 100.0 * (dur - remaining.min(dur)) / dur;
    match pct - elapsed_pct {
        d if d > 5.0 => "#ff5555",
        d if d < -5.0 => "#8be9fd",
        _ => "#50fa7b",
    }
}

fn remaining_str(remaining: f64) -> String {
    let s = remaining as u64;
    match s {
        0 => "now".into(),
        _ if s >= 86_400 => format!("{}d {}h", s / 86_400, (s % 86_400) / 3600),
        _ if s >= 3600 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        _ => format!("{}m", s / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(percent: Option<f64>) -> Window {
        Window {
            percent,
            resets_at: None,
        }
    }

    #[test]
    fn bar_color_red_when_over_elapsed() {
        assert_eq!(bar_color(100.0, 10.0, 100.0), "#ff5555");
    }

    #[test]
    fn bar_color_cyan_when_under_elapsed() {
        assert_eq!(bar_color(80.0, 0.0, 100.0), "#8be9fd");
    }

    #[test]
    fn bar_color_green_on_pace() {
        assert_eq!(bar_color(70.0, 30.0, 100.0), "#50fa7b");
    }

    #[test]
    fn remaining_str_formats() {
        assert_eq!(remaining_str(0.0), "now");
        assert_eq!(remaining_str(59.0), "0m");
        assert_eq!(remaining_str(90.0), "1m");
        assert_eq!(remaining_str(3_600.0), "1h 0m");
        assert_eq!(remaining_str(5_400.0), "1h 30m");
        assert_eq!(remaining_str(86_400.0), "1d 0h");
        assert_eq!(remaining_str(90_000.0), "1d 1h");
    }

    #[test]
    fn fmt_window_na_when_no_percent() {
        let out = fmt_window("5h", &window(None), 18_000.0);
        assert!(out.contains("n/a"));
    }

    #[test]
    fn fmt_window_renders_percent() {
        let out = fmt_window("30d", &window(Some(80.0)), 2_592_000.0);
        assert!(out.contains("<b>30d</b>"));
        assert!(out.contains("80%"));
        assert!(out.contains("now"));
    }

    #[test]
    fn fmt_window_clamps_percent() {
        let out = fmt_window("7d", &window(Some(150.0)), 604_800.0);
        assert!(out.contains("100%"));
    }

    #[test]
    fn parse_accepts_valid_body() {
        let body = r#"{"usage":{"monthly":{"percent":12.5},"weekly":{"percent":null},"rolling":{"percent":3.1}}}"#;
        assert!(parse(body).is_some());
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"usage":{}}"#).is_none());
    }

    #[test]
    fn render_contains_all_windows() {
        let u = Usage {
            monthly: window(Some(50.0)),
            weekly: window(Some(50.0)),
            rolling: window(Some(50.0)),
        };
        let out = render(&u);
        assert!(out.contains("OpenCode Go"));
        assert!(out.contains("5h"));
        assert!(out.contains("7d"));
        assert!(out.contains("30d"));
    }
}
