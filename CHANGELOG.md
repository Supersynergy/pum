# Changelog

All notable changes to pum are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Changed — rewritten in Rust (2026-06-03)
- **pum is now a single static Rust binary** (`apps/pum/`, edition 2024; clap + rusqlite + serde + rayon). The Python v0 (verified, 718-pkg scan) is replaced — it depended on the python runtime it manages (uv/pipx), which a machine-maintenance tool must not. Rust matches the direct peers (topgrade, mise, uv) and installs via `cargo install --path apps/pum` (→ `~/.cargo/bin/pum`), no venv.
- Full parity verified on host: 12 adapters (no OS), `scan` = 721 pkgs (≈ python 718), `check` finds updates, `report`/`update --dry-run`/`self`/`doctor` all work, 6 unit tests pass, `cargo clippy` + release build clean, 0 OS references.
- Inventory moved to `~/.local/share/pum/inventory.db` (+ `inventory.json`); override with `$PUM_DB`. Same schema + PK `(manager,name,installed)` + status-preserving upsert as the hardened Python version.
- Python reference preserved in git history (commit `3f65213`).


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
