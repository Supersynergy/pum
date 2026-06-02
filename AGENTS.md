# pum — Agent Instructions

Entry point: `apps/pum/pum.py` (single file, stdlib only).
Tests: `python3 -m unittest discover -s apps/pum/tests -p "test_*.py" -v`
Lint: `ruff check apps/pum/`

## Adapter pattern

Add new managers by subclassing `Adapter` in `pum.py` and appending an instance to `ALL_ADAPTERS`.
Every subprocess call must go through `_run(argv, timeout=N)` — never use subprocess directly.
Updates are never triggered in `scan` or `check`.

## Key paths

- `data/inventory.db` — SQLite inventory (auto-created)
- `data/inventory.json` — JSON mirror (written on scan)
- `docs/SPEC.md` — adapter contract and data model
