//! Terminal presenter — owns all ANSI vt100 escape sequences and `is_terminal()`
//! detection so the application/domain crates stay IO-free.
//!
//! Layout: dim `[cc-resume]` prefix, banner box at start-up, idle spinner line
//! that overwrites itself each second, countdown progress bar with
//! green → yellow → red colour escalation as the reset moment approaches.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use clw_application::ports::{BannerInfo, IdleInfo, Presenter};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BAR_FULL: &str = "█";
const BAR_EMPTY: &str = "░";

const CLEAR_LINE: &str = "\r\x1b[K";
const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const BRIGHT_YELLOW: &str = "\x1b[93m";
const BRIGHT_RED: &str = "\x1b[91m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const BOLD_BRIGHT_GREEN: &str = "\x1b[1;92m";

const NON_TTY_TICK_SECS: i64 = 900; // log every 15 min in non-TTY mode

#[derive(Debug)]
pub struct TerminalPresenter {
    is_tty: bool,
    spinner_idx: AtomicUsize,
    last_non_tty_tick_at: AtomicI64,
}

impl Default for TerminalPresenter {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalPresenter {
    #[must_use]
    pub fn new() -> Self {
        let is_tty = io::stdout().is_terminal();
        if is_tty {
            print!("{HIDE_CURSOR}");
            let _ = io::stdout().flush();
        }
        Self {
            is_tty,
            spinner_idx: AtomicUsize::new(0),
            last_non_tty_tick_at: AtomicI64::new(0),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.is_tty {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    fn prefix(&self) -> String {
        self.paint(DIM, "[cc-resume]")
    }

    fn next_spinner(&self) -> &'static str {
        let i = self.spinner_idx.fetch_add(1, Ordering::Relaxed);
        SPINNER[i % SPINNER.len()]
    }

    fn write_log(&self, glyph_color: &str, glyph: &str, body: &str) {
        let mut stdout = io::stdout().lock();
        let prefix = self.prefix();
        if self.is_tty {
            writeln!(
                stdout,
                "{CLEAR_LINE}{prefix} {glyph_color}{glyph}{RESET} {body}"
            )
            .ok();
        } else {
            writeln!(stdout, "[cc-resume] {glyph} {body}").ok();
        }
    }

    fn write_log_err(&self, glyph_color: &str, glyph: &str, body: &str) {
        let mut stderr = io::stderr().lock();
        let prefix = self.prefix();
        if self.is_tty {
            writeln!(
                stderr,
                "{CLEAR_LINE}{prefix} {glyph_color}{glyph}{RESET} {body}"
            )
            .ok();
        } else {
            writeln!(stderr, "[cc-resume] {glyph} {body}").ok();
        }
    }
}

impl Drop for TerminalPresenter {
    fn drop(&mut self) {
        if self.is_tty {
            print!("{CLEAR_LINE}{SHOW_CURSOR}");
            let _ = io::stdout().flush();
        }
    }
}

impl Presenter for TerminalPresenter {
    fn banner(&self, info: &BannerInfo) {
        if !self.is_tty {
            println!(
                "[cc-resume-session v{}] watching session '{}' (poll {}s, buffer {}s)",
                info.version, info.session, info.poll_interval_seconds, info.buffer_seconds
            );
            return;
        }
        let bar = self.paint(
            BOLD_CYAN,
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        );
        let dim = |s: &str| self.paint(DIM, s);
        let bold = |s: &str| self.paint(BOLD, s);
        let bg_title = self.paint(BOLD_BRIGHT_GREEN, "🛡  cc-resume-session");
        let v = dim(&format!("v{}", info.version));
        println!();
        println!("   {bar}");
        println!("    {bg_title}  {v}");
        println!("   {bar}");
        println!("    {}    {}", dim("session"), bold(&info.session));
        println!(
            "    {}       every {}",
            dim("poll"),
            bold(&format!("{}s", info.poll_interval_seconds))
        );
        println!(
            "    {}     {} after reset",
            dim("buffer"),
            bold(&format!("+{}s", info.buffer_seconds))
        );
        println!("    {}    {}", dim("trigger"), dim(&info.limit_phrase));
        println!(
            "    {}   {}",
            dim("response"),
            self.paint(BOLD_BRIGHT_GREEN, &info.resume_text)
        );
        println!("   {bar}");
        println!();
    }

    fn started(&self) {
        let now = format_clock_now();
        self.write_log(GREEN, "✓", &format!("started · {now}"));
    }

    fn idle_tick(&self, info: &IdleInfo) {
        if !self.is_tty {
            return;
        }
        let frame = self.next_spinner();
        let clock = format_clock(info.now_epoch);
        let since_poll = format_short(info.now_epoch - info.last_poll_at);
        let uptime = format_short(info.now_epoch - info.started_at);
        let mut stdout = io::stdout().lock();
        write!(
            stdout,
            "{CLEAR_LINE}{prefix} {cyan_frame}  {bold_watching}  {tail}",
            prefix = self.prefix(),
            cyan_frame = self.paint(CYAN, frame),
            bold_watching = self.paint(BOLD, "watching"),
            tail = self.paint(
                DIM,
                &format!(
                    "· clock {clock} · last poll {since_poll} ago · uptime {uptime} · resumes {}",
                    info.resume_count
                )
            ),
        )
        .ok();
        let _ = stdout.flush();
    }

    fn limit_detected(&self, target_human: &str, wait_seconds: i64, buffer_seconds: i64) {
        let now = format_clock_now();
        let body = format!(
            "{now}  limit detected — reset at {bold_target}, waiting {wait} (buffer +{buffer_seconds}s)",
            bold_target = self.paint(BOLD, target_human),
            wait = self.paint(BOLD, &format_short(wait_seconds)),
        );
        self.write_log(MAGENTA, "⚡", &body);
    }

    fn limit_already_passed(&self, target_human: &str) {
        let now = format_clock_now();
        let body = format!(
            "{now}  limit already cleared ({bold_target} is in the past) — sending now",
            bold_target = self.paint(BOLD, target_human),
        );
        self.write_log(MAGENTA, "⚡", &body);
    }

    fn countdown_step(&self, remaining_seconds: i64, total_seconds: i64, target_human: &str) {
        if !self.is_tty {
            // Plain mode: log roughly every NON_TTY_TICK_SECS.
            let now = chrono_now();
            let last = self.last_non_tty_tick_at.load(Ordering::Relaxed);
            if now - last >= NON_TTY_TICK_SECS {
                self.last_non_tty_tick_at.store(now, Ordering::Relaxed);
                println!(
                    "[cc-resume] {} remaining (target {target_human})",
                    format_hms(remaining_seconds)
                );
            }
            return;
        }
        let total = total_seconds.max(1);
        let elapsed = (total - remaining_seconds).max(0);
        let pct = (elapsed * 100 / total).clamp(0, 100);
        let bar = render_bar(elapsed, total, 22);
        let color = remaining_color(remaining_seconds);
        let mut stdout = io::stdout().lock();
        write!(
            stdout,
            "{CLEAR_LINE}{prefix} {wait_glyph}  resume in {remaining}  {target}  {bar_part}",
            prefix = self.prefix(),
            wait_glyph = self.paint(MAGENTA, "⏳"),
            remaining = self.paint(&format!("{BOLD}{color}"), &format_hms(remaining_seconds)),
            target = self.paint(DIM, &format!("· target {target_human}")),
            bar_part = self.paint(DIM, &format!("{bar} {pct}%")),
        )
        .ok();
        let _ = stdout.flush();
    }

    fn resumed(&self, count: u32, resume_text: &str, session: &str) {
        let now = format_clock_now();
        let body = format!(
            "{now}  sent {bold_text} → {bold_session}  {dim_count}",
            bold_text = self.paint(BOLD, &format!("'{resume_text}'")),
            bold_session = self.paint(BOLD, session),
            dim_count = self.paint(DIM, &format!("(resume #{count})")),
        );
        self.write_log(GREEN, "✓", &body);
    }

    fn shutdown(&self, uptime_seconds: i64, total_resumes: u32) {
        // Make sure the last spinner/countdown line is wiped before printing summary.
        if self.is_tty {
            print!("{CLEAR_LINE}");
            let _ = io::stdout().flush();
        }
        let body = format!(
            "shutdown · uptime {bold_uptime} · resumes triggered: {bold_count}",
            bold_uptime = self.paint(BOLD, &format_hms(uptime_seconds)),
            bold_count = self.paint(BOLD, &total_resumes.to_string()),
        );
        self.write_log(BOLD, "▸", &body);
    }

    fn warn(&self, message: &str) {
        self.write_log_err(YELLOW, "⚠", message);
    }

    fn error(&self, message: &str) {
        self.write_log_err(RED, "✗", message);
    }
}

// ---------- formatting helpers ----------

fn format_hms(seconds: i64) -> String {
    let s = seconds.max(0);
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn format_short(seconds: i64) -> String {
    let s = seconds.max(0);
    if s >= 3600 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

fn format_clock(epoch: i64) -> String {
    jiff::Timestamp::from_second(epoch).map_or_else(
        |_| "??:??:??".to_string(),
        |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M:%S")
                .to_string()
        },
    )
}

fn format_clock_now() -> String {
    format_clock(chrono_now())
}

fn chrono_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    i64::try_from(secs).unwrap_or(i64::MAX)
}

fn remaining_color(seconds: i64) -> &'static str {
    if seconds > 1800 {
        BRIGHT_GREEN
    } else if seconds > 300 {
        BRIGHT_YELLOW
    } else {
        BRIGHT_RED
    }
}

fn render_bar(done: i64, total: i64, width: usize) -> String {
    let total = total.max(1);
    let width_i64 = i64::try_from(width).unwrap_or(i64::MAX);
    let raw = (done.max(0) * width_i64) / total;
    let clamped = raw.clamp(0, width_i64);
    let filled = usize::try_from(clamped).unwrap_or(0);
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", BAR_FULL.repeat(filled), BAR_EMPTY.repeat(empty))
}
