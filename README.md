# pum — Package Update Manager

Unified multi-manager package inventory and update tool.
Adapter-based: only managers present on your PATH are activated.

## Quick Start

```bash
python3 apps/pum/pum.py doctor   # which managers are live
python3 apps/pum/pum.py scan     # inventory all installed packages
python3 apps/pum/pum.py check    # find outdated packages
python3 apps/pum/pum.py report   # print table
python3 apps/pum/pum.py report --outdated --json
python3 apps/pum/pum.py update --dry-run --all
python3 apps/pum/pum.py self     # show manager self-update commands
python3 apps/pum/pum.py self --apply
```

Or via just:

```bash
just doctor
just scan
just check
just report
just report --outdated
just update --dry-run
just test
just lint
```

## Adapters

brew · npm · pnpm · bun · uv · pipx · cargo · rustup · gem · go · mise · gh · softwareupdate (macOS, report-only)

## Data

- `data/inventory.db` — SQLite, table `tools(manager, name, installed, latest, status, source, checked_at)`
- `data/inventory.json` — JSON mirror, written on every `pum scan`

## Requirements

Python 3.11+ · No third-party dependencies (stdlib only).
Optional: `uv` for `uv sync` / console entry install; `ruff` for linting.
