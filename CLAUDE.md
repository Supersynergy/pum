# pum — Claude Instructions

Single-file Python CLI at `apps/pum/pum.py`. Stdlib only. Python 3.11+.

## Gates (run after any edit)

```bash
python3 -m unittest discover -s apps/pum/tests -p "test_*.py" -v
python3 apps/pum/pum.py doctor
```

## Rules

- All subprocess calls via `_run(argv, timeout=N)` — never hang.
- Adapters catch all exceptions and return `[]` on failure.
- Never trigger updates in `scan` or `check`.
- `softwareupdate` is macOS report-only; never auto-run upgrade.
- New adapters: subclass `Adapter`, append to `ALL_ADAPTERS`.
