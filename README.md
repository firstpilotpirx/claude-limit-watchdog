# claude-limit-watchdog

> Watchdog that keeps Claude Code running across rate-limit windows — detects the limit, waits for reset, and resumes work automatically.

Claude Code stops with a `You've hit your limit · resets 3:50am (Europe/Belgrade)` message when you hit your usage cap. Normally you have to come back at the reset time and type something to continue. This tool does that for you.

Run it once against your tmux session and forget. Survives unlimited limit hits in a row — perfect for long unattended runs (overnight refactors, big migrations, batch tasks).

---

## How it works

1. **Watch.** Every 60s captures the last 200 lines of the tmux pane and greps for `You've hit your limit`.
2. **Parse.** Extracts the reset time and timezone from formats like `resets 3:50am (Europe/Belgrade)`, `resets 15:50 (UTC)`, or `resets 3am (UTC)` — supports 12/24-hour, am/pm, any IANA timezone.
3. **Wait.** Sleeps until the reset moment + 60s buffer (so the limit is definitely cleared). On a TTY shows a live progress bar with green→yellow→red colour escalation as the moment approaches.
4. **Resume.** Sends `continue the work where you left off` + Enter to the pane. Claude Code treats this as normal user input and picks up where it stopped.
5. **Loop.** Doesn't exit after one resume — keeps watching, ready for the next limit. Deduplicates so the same limit message doesn't trigger twice.

Exits on Ctrl-C (prints uptime + total resumes triggered) or if the tmux session disappears.

---

## Requirements

- Rust **1.85+** (pinned via `rust-toolchain.toml` — `rustup` will install it automatically on first build)
- `tmux`
- A running tmux session with Claude Code inside

---

## Build

```bash
git clone https://github.com/<your-user>/claude-limit-watchdog.git
cd claude-limit-watchdog
cargo build --release
```

The binary lands at `target/release/cc-resume-session` (~2.5 MB, statically linked except for libc).

For day-to-day work prefer `cargo build` (debug) or `cargo check` (just type-check, fastest).

---

## Install

Three options, pick whichever fits.

**1. Cargo install (recommended for users):**

```bash
cargo install --path crates/cli
# binary → ~/.cargo/bin/cc-resume-session
```

**2. Manual copy (for quick local install):**

```bash
cargo build --release
install -m 755 target/release/cc-resume-session ~/.local/bin/
```

**3. Symlink from the build tree (handy during development):**

```bash
ln -s "$PWD/target/release/cc-resume-session" ~/.local/bin/cc-resume-session
```

Make sure the install directory is on your `$PATH`.

---

## Usage

Start Claude Code in a named tmux session:

```bash
tmux new -s work
# inside tmux:
claude
```

In another shell, point the watchdog at it:

```bash
cc-resume-session work
```

Help and version:

```bash
cc-resume-session --help
cc-resume-session --version
```

The watchdog stays in the foreground — keep it running in another tmux pane / terminal tab. Ctrl-C exits cleanly.

---

## Configuration

The watchdog itself takes a single positional argument (the tmux session name). Behavioural defaults are baked in but easy to change.

### Built-in defaults

| Setting          | Default                                | Meaning                                       |
| ---------------- | -------------------------------------- | --------------------------------------------- |
| Poll interval    | `60s`                                  | Seconds between pane checks                   |
| Buffer           | `60s`                                  | Extra wait after the reset moment             |
| Limit phrase     | `You've hit your limit`                | Substring that triggers detection             |
| Resume text      | `continue the work where you left off` | Text sent to the pane after reset             |
| Pane scan window | last `200` lines                       | How many trailing lines of the pane to scan   |

These live in `crates/application/src/watch_service.rs::WatchConfig::defaults_for`. Change them there and rebuild — there are no CLI flags or env vars for them yet (the bash original didn't have them either).

### Environment variables

| Variable      | Effect                                                                                                                                                                      |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RUST_LOG`    | Standard `tracing` filter, e.g. `RUST_LOG=debug cc-resume-session work`. Output goes to stderr so it doesn't fight the live status line on stdout. Defaults to `warn`.      |
| `NO_COLOR`    | Not currently honoured — the presenter detects `is_terminal()` and skips ANSI when piped, but doesn't read `NO_COLOR`. Add a check in `TerminalPresenter::new` if you need. |

### Project-level config files

| File                    | What it controls                                                                              |
| ----------------------- | --------------------------------------------------------------------------------------------- |
| `Cargo.toml`            | Workspace + shared package metadata + shared dependency versions + workspace-wide lints       |
| `rust-toolchain.toml`   | Pinned Rust version + components (`rustfmt`, `clippy`, `llvm-tools-preview`)                  |
| `rustfmt.toml`          | Formatter rules                                                                               |
| `.taplo.toml`           | TOML formatter rules                                                                          |
| `deny.toml`             | License whitelist, vulnerability gates, **architecture rules** (`[bans]` — see below)         |
| `Justfile`              | Task runner recipes (build/check/test/clippy/etc.)                                            |
| `.github/workflows/ci.yml` | CI pipeline (matches the local `just` recipes)                                             |

---

## Project layout

```
claude-limit-watchdog/
├── Cargo.toml                    workspace root
├── rust-toolchain.toml
├── rustfmt.toml  .taplo.toml  deny.toml  Justfile
├── crates/
│   ├── domain/                   pure types + parser + time math (no IO)
│   ├── application/              ports (traits) + WatchService use case
│   ├── infrastructure/           adapters: TmuxCli, SystemClock, CtrlCStop, TerminalPresenter
│   └── cli/                      composition root, binary `cc-resume-session`
└── .github/workflows/ci.yml
```

**Layering rules** are enforced two ways:
- The compiler refuses imports that aren't declared in `Cargo.toml`. `domain` doesn't depend on `application`, so application types are physically unreachable from domain code.
- `cargo deny check bans` — see `deny.toml` — locks IO crates (`ctrlc`, `clap`, `tracing-subscriber`) into specific layers. If `domain` accidentally pulls one in, CI fails.

---

## Development

Day-to-day commands via `just`. Install once: `cargo install just`.

```bash
just check        # cargo check --workspace --all-targets   (fastest type-check)
just test         # cargo nextest run --workspace            (or: cargo test)
just clippy       # cargo clippy --workspace -- -D warnings
just lint         # fmt-check + clippy + taplo
just fmt          # apply rustfmt
just arch         # cargo deny check bans   (architecture rules)
just deny         # full cargo deny check   (bans + advisories + licenses + sources)
just audit        # cargo audit             (RustSec CVE feed)
just coverage     # cargo llvm-cov --workspace --lcov
just doc          # cargo doc --no-deps -D warnings
just assemble     # cargo build --workspace --release
just pipeline     # all of the above, in order — local equivalent of CI
```

One-time tooling install:

```bash
cargo install just cargo-nextest cargo-deny cargo-audit cargo-llvm-cov \
              cargo-machete cargo-outdated cargo-modules cargo-mutants taplo-cli
```

CI mirrors `just pipeline` — see `.github/workflows/ci.yml`.

---

## Notes

- The watchdog reads pane output via `tmux capture-pane`, so it doesn't need any Claude Code API access or login state — it's a pure UI-level integration.
- Reset time and timezone are taken from Claude's own message, so daylight-savings, custom timezones, and daily/weekly windows are all handled correctly by whatever Claude prints.
- If stdout isn't a TTY (e.g. piped to a log file) the live progress bar and spinner are suppressed; only milestone log lines are written.
