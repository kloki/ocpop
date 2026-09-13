use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::session;

#[derive(Deserialize)]
struct NiriWindow {
    id: u64,
    title: String,
}

pub fn pick() -> Result<()> {
    let sessions = session::list()?;
    if sessions.is_empty() {
        bail!("no opencode sessions");
    }

    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    let max_title = sessions
        .iter()
        .map(|s| s.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(70);

    for s in &sessions {
        let line = format!(
            "{} {} · {} · {}",
            s.state.glyph(),
            session::pad(&s.title, max_title),
            session::short_dir(&s.directory),
            session::relative_time(s.updated),
        );
        lines.push(line.clone());
        mapping.push((line, s));
    }

    let selected = fuzzel(&lines)?;
    match mapping.iter().find(|(l, _)| *l == selected) {
        Some((_, s)) => open(s),
        None => Ok(()),
    }
}

fn fuzzel(lines: &[String]) -> Result<String> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let input = lines.join("\n");
    let mut child = Command::new("fuzzel")
        .args(["--dmenu", "--log-level=none"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("launching fuzzel")?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .context("writing to fuzzel")?;

    let out = child.wait_with_output()?;
    let selected = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if selected.is_empty() {
        // canceled (ESC) or no match
        bail!("selection canceled");
    }
    Ok(selected)
}

fn open(s: &session::Session) -> Result<()> {
    if let Some(id) = find_window(&s.title)? {
        niri(&["msg", "action", "focus-window", "--id", &id.to_string()])?;
        return Ok(());
    }

    let dir = &s.directory;
    let script = format!("cd '{dir}' && exec opencode -s {}", s.id);
    std::process::Command::new("alacritty")
        .args([
            "--title",
            &format!("OC {}", s.id),
            "-e",
            "sh",
            "-c",
            &script,
        ])
        .spawn()
        .context("spawning alacritty")?;
    Ok(())
}

/// find a window whose title is exactly `OC | <session title>`
fn find_window(title: &str) -> Result<Option<u64>> {
    let out = niri_json(&["msg", "--json", "windows"])?;
    let wanted = format!("OC | {title}");
    let windows: Vec<NiriWindow> = serde_json::from_slice(&out)?;
    Ok(windows
        .into_iter()
        .find(|w| w.title == wanted)
        .map(|w| w.id))
}

fn niri(args: &[&str]) -> Result<Vec<u8>> {
    let out = std::process::Command::new("niri")
        .args(args)
        .output()
        .context("running niri")?;
    if !out.status.success() {
        bail!("niri failed: {:?}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(out.stdout)
}

fn niri_json(args: &[&str]) -> Result<Vec<u8>> {
    niri(args)
}
