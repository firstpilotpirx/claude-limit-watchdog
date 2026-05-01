//! Terminal presenter — owns all ANSI vt100 escape sequences, `is_terminal()`
//! detection, and `comfy-table` rendering so the application/domain layers
//! stay IO-free.
//!
//! Layout in TTY mode:
//!
//!   ┌───────────────────────────────────────┐
//!   │ Plan usage · session 14:23 → 19:23 …  │
//!   │ ┌─ Model ─┬ Turns ┬ Input ┬ Output ┐  │   ← live "panel"
//!   │ │ opus    │ 127   │ 3.2k  │ 84.5k  │  │     redrawn each tick
//!   │ └─────────┴───────┴───────┴────────┘  │
//!   │                                       │
//!   │ [cc-resume] ⠋ watching · clock …      │   ← bottom status line
//!   └───────────────────────────────────────┘
//!
//! Permanent log lines (banner, "started", "resumed", warnings) wipe the panel
//! first, write themselves, and let the next tick re-draw the panel below.

use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use clw_watchdog_core::application::ports::{BannerInfo, IdleInfo, Presenter};
use clw_watchdog_core::domain::{ModelStats, SessionWindow};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use unicode_width::UnicodeWidthChar;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const BAR_FULL: &str = "█";
const BAR_EMPTY: &str = "░";

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

const NON_TTY_TICK_SECS: i64 = 900;

/// Claude Code's plan tracks usage in 5-hour rolling windows; the resume
/// countdown bar is scaled to that window so the fill ratio reads as
/// "how far through the current window are we", regardless of how late
/// in the window the limit was hit.
const SESSION_WINDOW_SECS: i64 = 5 * 3600;

#[derive(Debug)]
pub struct TerminalPresenter {
    is_tty: bool,
    spinner_idx: AtomicUsize,
    last_non_tty_tick_at: AtomicI64,
    /// How many newlines the last live panel emitted. Used to scroll back up
    /// before re-rendering or before printing a permanent log line.
    last_panel_lines: AtomicUsize,
    /// Snapshot of the terminal's `stty` settings captured at start-up.
    /// Restored in `Drop` so the user's shell isn't left in raw mode if we
    /// crash or exit normally. `None` means we're not on a TTY (or the save
    /// failed) and there's nothing to restore.
    saved_stty: Option<String>,
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
        let saved_stty = if is_tty {
            let saved = save_stty();
            if saved.is_some() {
                // -echo: stray keystrokes won't appear on the live panel.
                // -icanon: Enter is just a byte, not "newline + cursor down",
                //          so accidental input doesn't push our panel up.
                set_silent_input();
            }
            saved
        } else {
            None
        };
        if is_tty {
            print!("{HIDE_CURSOR}");
            let _ = io::stdout().flush();
        }
        Self {
            is_tty,
            spinner_idx: AtomicUsize::new(0),
            last_non_tty_tick_at: AtomicI64::new(0),
            last_panel_lines: AtomicUsize::new(0),
            saved_stty,
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

    /// Re-draw the live panel in place. `body` may contain newlines; the
    /// **last** line stays on screen until the next call.
    ///
    /// Each visible line is truncated to the current terminal width so it
    /// can't trigger a soft-wrap — soft-wraps fool our cursor bookkeeping
    /// (we count `\n` chars, not wrapped rows) and the panel "scrolls"
    /// instead of redrawing.
    ///
    /// As a belt-and-suspenders against off-by-one width math (e.g. an emoji
    /// whose display width we miscounted), DECAWM (auto-wrap mode) is turned
    /// off for the duration of the panel write and restored after — a line
    /// that hits the right margin is silently clipped instead of pushing
    /// the cursor onto the next row.
    fn render_panel(&self, body: &str) {
        if !self.is_tty {
            return;
        }
        let body = truncate_lines_to_width(body, terminal_cols());
        let mut stdout = io::stdout().lock();
        let prev = self.last_panel_lines.load(Ordering::Relaxed);
        if prev > 0 {
            write!(stdout, "\x1b[{prev}A\r\x1b[J").ok();
        } else {
            write!(stdout, "\r\x1b[K").ok();
        }
        write!(stdout, "\x1b[?7l").ok();
        write!(stdout, "{body}").ok();
        write!(stdout, "\x1b[?7h").ok();
        let new_lines = body.bytes().filter(|&b| b == b'\n').count();
        self.last_panel_lines.store(new_lines, Ordering::Relaxed);
        let _ = stdout.flush();
    }

    /// Wipe the live panel before writing a permanent (scrolling) line.
    fn clear_panel(&self) {
        if !self.is_tty {
            return;
        }
        let mut stdout = io::stdout().lock();
        let prev = self.last_panel_lines.load(Ordering::Relaxed);
        if prev > 0 {
            write!(stdout, "\x1b[{prev}A\r\x1b[J").ok();
        } else {
            write!(stdout, "\r\x1b[K").ok();
        }
        self.last_panel_lines.store(0, Ordering::Relaxed);
        let _ = stdout.flush();
    }

    fn write_log(&self, glyph_color: &str, glyph: &str, body: &str) {
        self.clear_panel();
        let mut stdout = io::stdout().lock();
        if self.is_tty {
            writeln!(
                stdout,
                "{} {glyph_color}{glyph}{RESET} {body}",
                self.prefix()
            )
            .ok();
        } else {
            writeln!(stdout, "[cc-resume] {glyph} {body}").ok();
        }
        let _ = stdout.flush();
    }

    fn write_log_err(&self, glyph_color: &str, glyph: &str, body: &str) {
        self.clear_panel();
        let mut stderr = io::stderr().lock();
        if self.is_tty {
            writeln!(
                stderr,
                "{} {glyph_color}{glyph}{RESET} {body}",
                self.prefix()
            )
            .ok();
        } else {
            writeln!(stderr, "[cc-resume] {glyph} {body}").ok();
        }
        let _ = stderr.flush();
    }

    fn build_idle_panel(&self, info: &IdleInfo) -> String {
        let mut out = String::new();
        if !info.session_stats.is_empty() {
            if let Some(window) = info.session_window {
                out.push_str(&self.session_header(&window));
                out.push('\n');
                out.push_str(&self.session_progress_line(&window, info.now_epoch));
                out.push('\n');
            }
            out.push_str(&render_stats_table(&info.session_stats, self.is_tty));
            out.push('\n');
            out.push('\n');
        }
        out.push_str(&self.idle_status_line(info));
        out
    }

    /// Sub-line under the session header showing the rolling 5h window's
    /// hard reset point and a progress bar of where we are inside it.
    /// Same red→green colour scheme as the resume countdown so both bars
    /// read the same way: empty/red = lots of time before reset,
    /// full/green = reset is imminent.
    fn session_progress_line(&self, window: &SessionWindow, now: i64) -> String {
        let total = (window.end_epoch - window.start_epoch).max(1);
        let elapsed = (now - window.start_epoch).clamp(0, total);
        let remaining = (window.end_epoch - now).max(0);
        let pct = (elapsed * 100 / total).clamp(0, 100);
        let fill = bar_fill_color(pct);
        let bar = render_colored_bar(elapsed, total, 22, fill);
        let reset_at = format_clock(window.end_epoch);
        format!(
            "{resets} {at}  {until}  {bar} {pct_text}",
            resets = self.paint(BOLD, "session resets"),
            at = self.paint(BOLD, &format!("at {reset_at}")),
            until = self.paint(DIM, &format!("· in {}", format_short(remaining))),
            pct_text = self.paint(DIM, &format!("{pct}%")),
        )
    }

    /// Header for the per-model usage table.
    ///
    /// Deliberately understated: we know precisely **what we observed in local
    /// Claude Code logs since the first activity of this run**. We do **not**
    /// know — and do not pretend to know — the actual Anthropic plan window or
    /// what fraction of the limit has been consumed. Anything from claude.ai
    /// web / Claude Desktop / other clients is invisible here.
    fn session_header(&self, window: &SessionWindow) -> String {
        let start_h = format_clock(window.start_epoch);
        format!(
            "{label}  {since}  {note}",
            label = self.paint(BOLD_CYAN, "Claude Code tokens this session"),
            since = self.paint(BOLD, &format!("· since {start_h} (local time)")),
            note = self.paint(DIM, "· web / Desktop not visible here"),
        )
    }

    fn idle_status_line(&self, info: &IdleInfo) -> String {
        let frame = self.next_spinner();
        let clock = format_clock(info.now_epoch);
        let since_poll = format_short(info.now_epoch - info.last_poll_at);
        let uptime = format_short(info.now_epoch - info.started_at);
        format!(
            "{prefix} {cyan_frame}  {bold_watching}  {tail}",
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
    }
}

impl Drop for TerminalPresenter {
    fn drop(&mut self) {
        if self.is_tty {
            self.clear_panel();
            print!("{SHOW_CURSOR}");
            let _ = io::stdout().flush();
        }
        if let Some(saved) = self.saved_stty.take() {
            restore_stty(&saved);
        }
    }
}

// ---------- terminal input mode helpers ----------

/// Capture the current `stty` settings so `Drop` can restore them.
///
/// `stty -g` prints a single colon-separated string (a portable "save file"
/// for terminal settings); pass it back unchanged and `stty` reapplies them.
fn save_stty() -> Option<String> {
    let out = Command::new("stty")
        .arg("-g")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Disable `echo` and canonical line buffering on the controlling terminal.
/// `-isig` is **not** set, so Ctrl-C still raises SIGINT (which our handler
/// catches).
fn set_silent_input() {
    let _ = Command::new("stty")
        .args(["-echo", "-icanon"])
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .status();
}

fn restore_stty(saved: &str) {
    let _ = Command::new("stty")
        .arg(saved)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .status();
}

/// Current terminal width in columns, or `None` if `stty size` fails or
/// returns garbage. Re-detected per render so window resizes are picked up
/// without a SIGWINCH handler.
fn terminal_cols() -> Option<usize> {
    let out = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut parts = s.split_whitespace();
    let _rows = parts.next()?;
    let cols: usize = parts.next()?.parse().ok()?;
    if cols == 0 {
        None
    } else {
        Some(cols)
    }
}

/// Truncate each `\n`-separated line of `body` to `max_cols` visible columns
/// (treating ANSI CSI escape sequences as zero-width). When `max_cols` is
/// `None` (couldn't detect the terminal) the body is returned unchanged —
/// preferable to mangling output on terminals where we can't measure.
fn truncate_lines_to_width(body: &str, max_cols: Option<usize>) -> String {
    let Some(max) = max_cols else {
        return body.to_string();
    };
    let mut out = String::with_capacity(body.len());
    let mut iter = body.split('\n').peekable();
    while let Some(line) = iter.next() {
        out.push_str(&truncate_visible(line, max));
        if iter.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

/// Cut a single line to at most `max_cols` visible columns. ANSI CSI escapes
/// (`ESC [ … letter`) are passed through without being counted; if a cut
/// happens mid-style we append `RESET` so the truncation doesn't bleed
/// colour into anything that follows.
///
/// Display width is computed via `UnicodeWidthChar::width` — emoji like `⏳`
/// occupy two columns, not one, and counting them as a single codepoint was
/// what let lines whose codepoint count matched the terminal width still
/// trigger an auto-wrap and tear the live panel apart.
fn truncate_visible(line: &str, max_cols: usize) -> String {
    let mut visible = 0usize;
    let mut in_esc = false;
    let mut had_escape = false;
    let mut out = String::with_capacity(line.len());
    for ch in line.chars() {
        if in_esc {
            out.push(ch);
            if ch.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_esc = true;
            had_escape = true;
            out.push(ch);
            continue;
        }
        let w = ch.width().unwrap_or(0);
        if visible + w > max_cols {
            if had_escape {
                out.push_str(RESET);
            }
            return out;
        }
        visible += w;
        out.push(ch);
    }
    out
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
        println!("    {}      {}", dim("quit"), dim("press q or Ctrl-C"));
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
        let panel = self.build_idle_panel(info);
        self.render_panel(&panel);
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

    fn countdown_step(&self, remaining_seconds: i64, target_human: &str) {
        if !self.is_tty {
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
        let elapsed = (SESSION_WINDOW_SECS - remaining_seconds).clamp(0, SESSION_WINDOW_SECS);
        let pct = (elapsed * 100 / SESSION_WINDOW_SECS).clamp(0, 100);
        let fill = bar_fill_color(pct);
        let bar = render_colored_bar(elapsed, SESSION_WINDOW_SECS, 22, fill);
        let color = remaining_color(remaining_seconds);
        let line = format!(
            "{prefix} {wait_glyph}  resume in {remaining}  {target}  {bar} {pct_text}",
            prefix = self.prefix(),
            wait_glyph = self.paint(MAGENTA, "⏳"),
            remaining = self.paint(&format!("{BOLD}{color}"), &format_hms(remaining_seconds)),
            target = self.paint(DIM, &format!("· target {target_human}")),
            pct_text = self.paint(DIM, &format!("{pct}%")),
        );
        self.render_panel(&line);
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
        self.clear_panel();
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

// ---------- comfy-table rendering ----------

fn header_cell(text: &str, align: CellAlignment, color: bool) -> Cell {
    let mut c = Cell::new(text).set_alignment(align);
    if color {
        c = c.add_attribute(Attribute::Bold).fg(Color::Cyan);
    }
    c
}

fn data_cell(text: String, align: CellAlignment, model_col: bool, color: bool) -> Cell {
    let mut c = Cell::new(text).set_alignment(align);
    if color {
        if model_col {
            c = c.fg(Color::Cyan);
        } else {
            c = c.fg(Color::White);
        }
    }
    c
}

fn total_cell(text: String, align: CellAlignment, color: bool) -> Cell {
    let mut c = Cell::new(text).set_alignment(align);
    if color {
        c = c.add_attribute(Attribute::Bold).fg(Color::Green);
    }
    c
}

fn render_stats_table(stats: &[ModelStats], color: bool) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        // Disabled — let columns size to content. Dynamic mode tries to fit
        // the terminal width and falls apart under PTY wrappers like `script`.
        .set_content_arrangement(ContentArrangement::Disabled)
        .set_header(vec![
            header_cell("Model", CellAlignment::Left, color),
            header_cell("Turns", CellAlignment::Right, color),
            // "Input" here is the **full** input the model processed:
            // input_tokens + cache_read_input_tokens + cache_creation_input_tokens.
            // Splitting them out is implementation detail of Anthropic's prompt cache.
            header_cell("Input", CellAlignment::Right, color),
            header_cell("Output", CellAlignment::Right, color),
            header_cell("Total", CellAlignment::Right, color),
        ]);

    let mut sum_turns = 0u64;
    let mut sum_input = 0u64;
    let mut sum_output = 0u64;

    for s in stats {
        let row_input = s.input_tokens + s.cache_read_input_tokens + s.cache_creation_input_tokens;
        let row_total = row_input + s.output_tokens;
        table.add_row(vec![
            data_cell(s.model.clone(), CellAlignment::Left, true, color),
            data_cell(fmt_compact(s.turns), CellAlignment::Right, false, color),
            data_cell(fmt_compact(row_input), CellAlignment::Right, false, color),
            data_cell(
                fmt_compact(s.output_tokens),
                CellAlignment::Right,
                false,
                color,
            ),
            data_cell(fmt_compact(row_total), CellAlignment::Right, false, color),
        ]);
        sum_turns += s.turns;
        sum_input += row_input;
        sum_output += s.output_tokens;
    }

    if stats.len() > 1 {
        let sum_total = sum_input + sum_output;
        table.add_row(vec![
            total_cell(String::from("TOTAL"), CellAlignment::Left, color),
            total_cell(fmt_compact(sum_turns), CellAlignment::Right, color),
            total_cell(fmt_compact(sum_input), CellAlignment::Right, color),
            total_cell(fmt_compact(sum_output), CellAlignment::Right, color),
            total_cell(fmt_compact(sum_total), CellAlignment::Right, color),
        ]);
    }

    table.to_string()
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

#[allow(clippy::cast_precision_loss)]
fn fmt_compact(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else if n >= 1_000 {
        let s = n.to_string();
        format!("{},{}", &s[..s.len() - 3], &s[s.len() - 3..])
    } else {
        n.to_string()
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

/// Fill colour for the countdown bar based on how full it is. Reads as a
/// traffic light: nearly empty = lots of waiting still ahead (red), nearly
/// full = reset is imminent (green). This is intentionally inverted from
/// `remaining_color`, which colours the *time text* by urgency.
fn bar_fill_color(pct: i64) -> &'static str {
    if pct >= 66 {
        BRIGHT_GREEN
    } else if pct >= 33 {
        BRIGHT_YELLOW
    } else {
        BRIGHT_RED
    }
}

fn render_colored_bar(done: i64, total: i64, width: usize, fill_color: &str) -> String {
    let total = total.max(1);
    let width_i64 = i64::try_from(width).unwrap_or(i64::MAX);
    let raw = (done.max(0) * width_i64) / total;
    let clamped = raw.clamp(0, width_i64);
    let filled = usize::try_from(clamped).unwrap_or(0);
    let empty = width.saturating_sub(filled);
    format!(
        "[{fill_color}{}{RESET}{DIM}{}{RESET}]",
        BAR_FULL.repeat(filled),
        BAR_EMPTY.repeat(empty),
    )
}
