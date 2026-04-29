//! `cc-resume-session` — composition root.
//!
//! Parses CLI args, loads/initialises settings, builds the concrete adapters
//! from `clw-watchdog`, wires them into the [`WatchService`], and runs the
//! watch loop.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clw_watchdog::{
    default_config_path, settings_store, wizard, ClaudeCodeLogReader, CtrlCStop, Presenter,
    Settings, SystemClock, TerminalPresenter, TmuxCli, WatchService,
};

#[derive(Debug, Parser)]
#[command(
    name    = "cc-resume-session",
    version,
    about   = "Watchdog that auto-resumes Claude Code after rate-limit windows.",
    long_about = None,
)]
struct Cli {
    /// Override the path to the YAML config file.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,

    /// Tmux session running Claude Code (when no subcommand is given).
    /// Required for the default `run` action.
    session: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the watchdog against a tmux session (this is the default).
    Run {
        /// Name of the tmux session running Claude Code.
        session: String,
    },
    /// Manage the on-disk configuration.
    #[command(subcommand)]
    Config(ConfigCmd),
}

#[derive(Debug, Subcommand)]
enum ConfigCmd {
    /// Run the interactive wizard. Pre-fills with current values if a config exists.
    Init,
    /// Print the current configuration and its file path.
    Show,
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let config_path = match cli.config {
        Some(p) => p,
        None => default_config_path().context("locate default config path")?,
    };

    match cli.command {
        Some(Command::Config(ConfigCmd::Init)) => cmd_config_init(&config_path),
        Some(Command::Config(ConfigCmd::Show)) => cmd_config_show(&config_path),
        Some(Command::Run { session }) => cmd_run(&config_path, &session),
        None => {
            let session = cli
                .session
                .context("missing tmux session name (run `cc-resume-session --help`)")?;
            cmd_run(&config_path, &session)
        }
    }
}

fn cmd_config_init(config_path: &std::path::Path) -> Result<()> {
    let existing = settings_store::load(config_path).context("load existing config")?;
    let settings = wizard::run(existing.as_ref())?;
    settings_store::save(config_path, &settings).context("save config")?;
    println!("\nConfiguration saved to {}", config_path.display());
    Ok(())
}

fn cmd_config_show(config_path: &std::path::Path) -> Result<()> {
    println!("Path: {}", config_path.display());
    match settings_store::load(config_path).context("load config")? {
        Some(s) => {
            let yaml = settings_store::to_yaml_string(&s).context("render config as yaml")?;
            print!("{yaml}");
        }
        None => println!("(no config file — run `cc-resume-session config init`)"),
    }
    Ok(())
}

fn cmd_run(config_path: &std::path::Path, session: &str) -> Result<()> {
    let settings = if let Some(s) =
        settings_store::load(config_path).context("load config")?
    {
        s
    } else {
        println!(
            "No config found at {}. Starting first-run wizard.",
            config_path.display()
        );
        let s = wizard::run(None)?;
        settings_store::save(config_path, &s).context("save config")?;
        println!("Configuration saved to {}\n", config_path.display());
        s
    };

    start_watchdog(settings, session.to_string())
}

fn start_watchdog(settings: Settings, session: String) -> Result<()> {
    let stop = CtrlCStop::install().context("install Ctrl-C handler")?;
    let presenter = TerminalPresenter::new();
    // Presenter just put the terminal into -icanon mode; safe to start the
    // single-keystroke 'q' watcher now (without -icanon the user would have
    // to press Enter after q, which still works but feels broken).
    stop.enable_q_to_quit();
    let usage_reader = ClaudeCodeLogReader::for_claude_dir(&settings.claude_dir);
    let cfg = settings.into_watch_config(session);
    let svc = WatchService::new(TmuxCli, SystemClock, stop, presenter, usage_reader, cfg);

    match svc.run() {
        Ok(stats) => {
            svc.presenter()
                .shutdown(stats.uptime_seconds, stats.resume_count);
            Ok(())
        }
        Err(e) => {
            svc.presenter().error(&format!("{e}"));
            Err(e.into())
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let _ = fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}
