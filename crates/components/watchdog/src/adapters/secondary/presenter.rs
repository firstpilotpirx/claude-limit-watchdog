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
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};

use crate::core::application::ports::{BannerInfo, IdleInfo, Presenter};
use crate::core::domain::{ModelStats, SessionWindow};

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

#[derive(Debug)]
pub struct TerminalPresenter {
    is_tty:               bool,
    spinner_idx:          AtomicUsize,
    last_non_tty_tick_at: AtomicI64,
    /// How many newlines the last live panel emitted. Used to scroll back up
    /// before re-rendering or before printing a permanent log line.
    last_panel_lines:     AtomicUsize,
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
            last_panel_lines: AtomicUsize::new(0),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.is_tty { format!("{code}{text}{RESET}") } else { text.to_string() }
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
    fn render_panel(&self, body: &str) {
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
        write!(stdout, "{body}").ok();
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
            writeln!(stdout, "{} {glyph_color}{glyph}{RESET} {body}", self.prefix()).ok();
        } else {
            writeln!(stdout, "[cc-resume] {glyph} {body}").ok();
        }
        let _ = stdout.flush();
    }

    fn write_log_err(&self, glyph_color: &str, glyph: &str, body: &str) {
        self.clear_panel();
        let mut stderr = io::stderr().lock();
        if self.is_tty {
            writeln!(stderr, "{} {glyph_color}{glyph}{RESET} {body}", self.prefix()).ok();
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
            }
            out.push_str(&render_stats_table(&info.session_stats, self.is_tty));
            out.push('\n');
            out.push('\n');
        }
        out.push_str(&self.idle_status_line(info));
        out
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

    fn countdown_step(&self, remaining_seconds: i64, total_seconds: i64, target_human: &str) {
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
        let total = total_seconds.max(1);
        let elapsed = (total - remaining_seconds).max(0);
        let pct = (elapsed * 100 / total).clamp(0, 100);
        let bar = render_bar(elapsed, total, 22);
        let color = remaining_color(remaining_seconds);
        let line = format!(
            "{prefix} {wait_glyph}  resume in {remaining}  {target}  {bar_part}",
            prefix = self.prefix(),
            wait_glyph = self.paint(MAGENTA, "⏳"),
            remaining = self.paint(&format!("{BOLD}{color}"), &format_hms(remaining_seconds)),
            target = self.paint(DIM, &format!("· target {target_human}")),
            bar_part = self.paint(DIM, &format!("{bar} {pct}%")),
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
        let row_input =
            s.input_tokens + s.cache_read_input_tokens + s.cache_creation_input_tokens;
        let row_total = row_input + s.output_tokens;
        table.add_row(vec![
            data_cell(s.model.clone(), CellAlignment::Left, true, color),
            data_cell(fmt_compact(s.turns), CellAlignment::Right, false, color),
            data_cell(fmt_compact(row_input), CellAlignment::Right, false, color),
            data_cell(fmt_compact(s.output_tokens), CellAlignment::Right, false, color),
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
        |t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M:%S").to_string(),
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
