# pum — Claude Instructions

Single static **Rust** binary at `apps/pum/` (edition 2024; clap + rusqlite + serde + rayon).
Install: `cargo install --path apps/pum` → `~/.cargo/bin/pum`.

## Gates (run after any edit, from `apps/pum/`)

```bash
cargo test
cargo clippy --all-targets -- -D warnings && cargo fmt --check
cargo run -q -- doctor
```

## Rules

- All subprocess calls via `run::run(argv, timeout)` — never hang, never panic.
- Adapters return `Vec<Package>` and never panic; `scan`/`check` wrap each in `catch_unwind`.
- Never trigger updates in `scan` or `check`.
- **Packages & tools only — never the OS.** No `softwareupdate`/OS adapter (reboot risk).
- New adapters: add a file in `src/adapters/`, impl the `Adapter` trait, register in `all_adapters()` (`mod.rs`). Keep a pure parse fn + a unit test in `main.rs`.
- Inventory: `~/.local/share/pum/inventory.db` (override `$PUM_DB`); PK `(manager,name,installed)`; re-scan must not wipe a prior `check` status.
