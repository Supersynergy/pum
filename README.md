# pum — Package Update Manager

Unified multi-manager package inventory and update tool. Single static **Rust** binary,
zero runtime deps. Adapter-based: only managers present on your PATH are activated.

## Install

```bash
cargo install --path apps/pum     # → ~/.cargo/bin/pum
# or:  just install
```

## Quick Start

```bash
pum doctor                 # which managers are live
pum scan                   # inventory all installed packages → SQLite + JSON
pum check                  # find outdated packages (queries each manager)
pum report                 # print table (installed vs latest)
pum report --outdated --json
pum update --dry-run --all # preview upgrades (mutates nothing)
pum update --manager brew  # upgrade one manager's packages
pum self                   # show manager self-update commands
pum self --apply
```

Inventory DB: `$PUM_DB` or `~/.local/share/pum/inventory.db` (+ `inventory.json`).

Dev via just (runs from source):

```bash
just build · just doctor · just scan · just check · just report --outdated · just test · just ci
```

## Why Rust

pum updates Python tooling (uv, pipx) — a Python implementation would depend on the very
runtime it manages. A self-contained Rust binary has zero runtime deps, matching the
direct peers (topgrade, mise, uv). See `CHANGELOG.md` (Python v0 → Rust port).

## Adapters

brew · npm · pnpm · bun · uv · pipx · cargo · rustup · gem · go · mise · gh

> Packages & developer tools only. The macOS OS updater (`softwareupdate`) is **excluded by design** — pum never scans or installs OS updates (reboot risk).

## Data

- `data/inventory.db` — SQLite, table `tools(manager, name, installed, latest, status, source, checked_at)`
- `data/inventory.json` — JSON mirror, written on every `pum scan`

## Requirements

Rust 1.85+ (edition 2024) to build · zero runtime deps (single static binary).
Build/lint via cargo: `cargo build --release`, `cargo clippy`, `cargo test`.
Adapters auto-activate only for managers present on your `PATH`.
