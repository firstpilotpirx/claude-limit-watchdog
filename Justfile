# Task runner — `just <recipe>`. Mirrors the Java/Gradle pipeline stages.
#
# One-time tooling install (run once on a fresh machine):
#   cargo install just cargo-nextest cargo-deny cargo-audit cargo-llvm-cov \
#                 cargo-machete cargo-outdated cargo-modules cargo-mutants taplo-cli

set shell := ["bash", "-uc"]

# --- aliases (short forms for daily use) ---
alias c   := check
alias t   := test
alias l   := lint
alias all := pipeline

# --- Group 1: build / lifecycle ---

clean:
    cargo clean

# `cargo check` is the fast type-check pass — equivalent to `compile-main`.
check:
    cargo check --workspace --all-targets

compile-tests:
    cargo test --workspace --no-run --all-targets

assemble:
    cargo build --workspace --release

# --- Group 2: formatting (spotless) ---

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

taplo:
    taplo fmt --check

taplo-fix:
    taplo fmt

# --- Group 3: linters / bug finders (pmd + spotbugs) ---

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Combined "lint" stage — fmt + clippy + taplo, like spotless + pmd + spotbugs.
lint: fmt-check clippy taplo

# --- Group 4: dependency hygiene ---

audit:
    cargo audit

deny:
    cargo deny check

machete:
    cargo machete

outdated:
    cargo outdated --workspace --root-deps-only

# --- Group 5: architecture (architecture-tests) ---
# Architecture is enforced primarily by:
#   1) crate boundaries (Cargo.toml deps) — the compiler refuses cycles & undeclared deps,
#   2) `cargo deny check bans` — IO libs are locked into specific crates (see deny.toml).

arch:
    cargo deny check bans

modules:
    cargo modules structure --package clw-cli

# --- Group 6: tests ---

# Unit + integration; nextest gives parallelism + JUnit XML.
test:
    cargo nextest run --workspace

test-doc:
    cargo test --workspace --doc

# --- Group 7: coverage (jacoco) ---

coverage:
    cargo llvm-cov --workspace --lcov --output-path lcov.info

coverage-html:
    cargo llvm-cov --workspace --html

# --- Group 8: docs ---

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# --- Group 9: bonuses ---

mutants:
    cargo mutants --workspace --in-place --no-shuffle

bench:
    cargo bench --workspace

# --- The full pipeline (parity with your 20-stage Gradle list) ---

pipeline: clean check fmt-check taplo clippy compile-tests assemble arch deny audit machete test test-doc coverage doc
    @echo "✓ pipeline green"
