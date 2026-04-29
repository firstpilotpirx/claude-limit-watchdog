# claude-limit-watchdog

> Watchdog that keeps Claude Code running across rate-limit windows — detects the limit, waits for reset, and resumes work automatically.

When Claude Code stops with `You've hit your limit · resets 3:50am (Europe/Belgrade)`, this tool waits until the reset moment and resumes the session for you. Perfect for overnight refactors and long unattended runs.

---

## Install

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git https://github.com/firstpilotpirx/claude-limit-watchdog
```

Binary lands at `~/.cargo/bin/cc-resume-session` — make sure it's on your `$PATH`.

> **Why the env var?** Cargo's built-in git client struggles with SSH agents and `insteadOf` rewrites in `~/.gitconfig`; the flag tells it to use the system `git` instead. To avoid retyping it, add to `~/.cargo/config.toml`:
> ```toml
> [net]
> git-fetch-with-cli = true
> ```

Requires Rust **1.85+** and `tmux`.

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

Ctrl-C exits cleanly and prints uptime + total resumes triggered.

---

## Configuration

Behavioural defaults (poll interval, buffer, limit phrase, resume text, scan window) live in
`crates/components/watchdog/src/core/application/watch_service.rs::WatchConfig::defaults_for` —
edit and rebuild. There are no CLI flags or env vars for them yet.

`RUST_LOG` is honoured: `RUST_LOG=debug cc-resume-session work` (logs go to stderr).

---

## Development

```bash
git clone https://github.com/firstpilotpirx/claude-limit-watchdog.git
cd claude-limit-watchdog
cargo build --release   # → target/release/cc-resume-session
```

Day-to-day tasks via `just` — see `Justfile` (`just check`, `just test`, `just lint`, `just pipeline`).

Hexagonal layout: `crates/apps/cli` is the composition root, `crates/components/watchdog` holds the feature (`core/{domain,application}` + `adapters/{primary,secondary}`). Layer boundaries are enforced by `cargo deny check bans` in `deny.toml`.
