# pum — Agent Instructions

Rust crate: `apps/pum/` (entry `src/main.rs`). Install: `cargo install --path apps/pum`.
Tests: `cd apps/pum && cargo test`. Lint: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`.

## Adapter pattern

Add a manager: new file in `src/adapters/<name>.rs`, impl the `Adapter` trait, register in
`all_adapters()` in `src/adapters/mod.rs`. Put parse logic in a pure `pub fn` + a unit test
in `main.rs`. Every subprocess call goes through `run::run(argv, timeout)` — never use
`std::process::Command` directly. Updates are never triggered in `scan` or `check`.
Packages & developer tools only — never add an OS updater.

## Key paths

- `~/.local/share/pum/inventory.duckdb` — DuckDB inventory + history (override `$PUM_DB`)
- `~/.local/share/pum/inventory.db` — legacy SQLite import source; never delete automatically
- `~/.local/share/pum/inventory.json` — JSON mirror (written on scan/refresh)
- `docs/SPEC.md` — adapter trait contract and data model
