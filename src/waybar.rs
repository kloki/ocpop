use std::io::Write;

use serde_json::json;

use crate::session::{self, State};

fn print(json: serde_json::Value) {
    // waybar or shapers may close the pipe; never panic on EPIPE
    let _ = writeln!(std::io::stdout(), "{json}");
}

pub fn run() -> anyhow::Result<()> {
    let sessions = session::list().unwrap_or_default();

    if sessions.is_empty() {
        // keep the usage-only fallback alive even with no sessions
        let u = crate::usage::status();
        let json = match u.tooltip {
            Some(tooltip) => json!({
                "text": "󱙺",
                "tooltip": tooltip,
                "class": if u.error.is_some() { "error" } else { "normal" },
            }),
            None => json!({
                "text": "󱙺?",
                "tooltip": u.error.unwrap_or_else(|| "usage unavailable".into()),
                "class": "error",
            }),
        };
        print(json);
        return Ok(());
    }

    let waiting = sessions
        .iter()
        .filter(|s| s.state == State::Waiting)
        .count();
    let running = sessions
        .iter()
        .filter(|s| s.state == State::Running)
        .count();
    let class = match (waiting, running) {
        (w, _) if w > 0 => "oc-waiting",
        (_, r) if r > 0 => "oc-running",
        _ => "normal",
    };

    let mut tooltip = crate::usage::status();
    let usage_block = match (tooltip.tooltip.take(), tooltip.error.take()) {
        (Some(html), _) => format!("{html}\n"),
        (None, Some(err)) => format!("<span color=\"#ff5555\">󱙺 {err}</span>\n"),
        (None, None) => String::new(),
    };

    let max_title = sessions
        .iter()
        .map(|s| s.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(50);
    let max_dir = sessions
        .iter()
        .map(|s| session::short_dir(&s.directory).chars().count())
        .max()
        .unwrap_or(0);

    let list = sessions
        .iter()
        .map(|s| {
            format!(
                "<span color=\"{color}\">{glyph}</span> <b>{title}</b> <i>{dir}</i> {sep} <b><i>{time}</i></b>",
                color = s.state.color(),
                glyph = s.state.glyph(),
                title = session::pad(&html_escape(&s.title), max_title),
                dir = session::pad(&html_escape(&session::short_dir(&s.directory)), max_dir),
                sep = "·",
                time = session::relative_time(s.updated),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let json = json!({
        "text": format!("{} {}", icon(&sessions), sessions.len()),
        "tooltip": format!("<tt>{usage_block}{list}</tt>"),
        "class": class,
    });
    print(json);
    Ok(())
}

fn icon(sessions: &[session::Session]) -> &'static str {
    if sessions.iter().any(|s| s.state == State::Waiting) {
        "󰚌"
    } else {
        "󱙺"
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
