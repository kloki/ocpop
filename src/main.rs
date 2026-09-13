mod pick;
mod session;
mod usage;
mod waybar;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ocpop", about = "opencode session widget for waybar")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// emit waybar status JSON (usage + sessions)
    Waybar,
    /// open a session picker (fuzzel) and focus/launch the picked session
    Pick,
    /// print the session list (debug view)
    List,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Waybar => waybar::run(),
        Cmd::Pick => pick::pick(),
        Cmd::List => list(),
    }
    .unwrap_or_else(|e| {
        eprintln!("✗ {e:#}");
        std::process::exit(1);
    });
}

fn list() -> anyhow::Result<()> {
    let sessions = session::list()?;
    let max_title = sessions
        .iter()
        .map(|s| s.title.chars().count())
        .max()
        .unwrap_or(0)
        .min(70);
    for s in &sessions {
        println!(
            "{} {} {} · {}",
            s.state.glyph(),
            session::pad(&s.title, max_title),
            session::short_dir(&s.directory),
            session::relative_time(s.updated),
        );
    }
    Ok(())
}
