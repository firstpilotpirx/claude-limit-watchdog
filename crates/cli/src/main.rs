//! `cc-resume-session` — composition root.
//!
//! Builds the concrete adapters from `clw-infrastructure`, wires them into the
//! `WatchService` from `clw-application`, and runs the watch loop.

use anyhow::{Context, Result};
use clap::Parser;
use clw_application::ports::Presenter;
use clw_application::{WatchConfig, WatchService};
use clw_infrastructure::{CtrlCStop, SystemClock, TerminalPresenter, TmuxCli};

#[derive(Debug, Parser)]
#[command(
    name    = "cc-resume-session",
    version,
    about   = "Watchdog that auto-resumes Claude Code after rate-limit windows.",
    long_about = None,
)]
struct Cli {
    /// Name of the tmux session running Claude Code.
    session: String,
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let stop = CtrlCStop::install().context("install Ctrl-C handler")?;
    let presenter = TerminalPresenter::new();
    let cfg = WatchConfig::defaults_for(cli.session);
    let svc = WatchService::new(TmuxCli, SystemClock, stop, presenter, cfg);

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
