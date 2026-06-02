# Changelog

All notable changes to pum are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Changed (2026-06-03)
- **Scope = packages & tools only, never the OS.** Removed the macOS `softwareupdate` adapter entirely (not just gated): pum no longer scans, reports, or installs OS updates — too dangerous (reboot risk). Adapter count 13 → 12. Verified: `doctor`, `scan`, and `update --all` contain zero softwareupdate references. The generic `report_only` safety net stays for any future risky manager (none today).

### Fixed (post-review hardening, 2026-06-03)
- **Safety:** `pum update --all` no longer auto-runs report-only managers. macOS `softwareupdate` (which can install OS updates / trigger reboots) is excluded from `--all` and requires an explicit `pum update --manager softwareupdate --apply`. Added `Adapter.report_only` + `--apply` flag on `update`.
- **Count invariant:** primary key changed to `(manager, name, installed)` so multiple versions of one name (e.g. mise `python` 3.12/3.13/3.14) each persist instead of collapsing to one DB row. Verified: 3 mise-python rows retained.
- **No status wipe:** a re-`scan` after `check` no longer resets `latest`/`status` to unknown — the upsert only overwrites when the incoming row carries a real value (verified: outdated flags preserved across re-scan).
- **UX:** `report --manager <bogus>` / empty filters now say "No packages match … (N in inventory)" instead of falsely claiming the inventory is empty.
- Lint clean (ruff: all checks passed); removed dead pipx parse variable.

### Added

- Initial pum implementation with adapter registry pattern.
- Adapters: brew, npm, pnpm, bun, uv, pipx, cargo, rustup, gem, go, mise, gh, softwareupdate.
- Commands: scan, check, report, update, self, doctor.
- SQLite inventory at `data/inventory.db`; JSON mirror at `data/inventory.json`.
- Concurrent adapter execution via `concurrent.futures.ThreadPoolExecutor`.
- Per-adapter timeout (60s default) on all subprocess calls.
- `--dry-run` flag for `pum update`.
- `--apply` flag for `pum self`.
- `--json`, `--outdated`, `--manager` flags for `pum report`.
- Unit tests for brew/npm/cargo/rustup/gem parse logic (no subprocess).
- `justfile` with setup, doctor, scan, check, report, update, self, test, lint, fmt, ci.
- `docs/SPEC.md` with adapter contract, data model, and command surface.
