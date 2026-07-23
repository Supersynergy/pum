# Changelog

All notable changes to pum are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

## [0.2.1] - 2026-07-23

### Fixed
- **pnpm global inventory is now JSON-only.** pnpm 10's tree-formatted output
  could be stored as fake package names such as `│` and `└──`; PUM now records
  the real package/version (`wrangler 4.114.0`) and removes those legacy rows.
- **A missing pnpm global manifest is an explicit freshness error.** Daily
  refreshes now say that pnpm freshness is unavailable instead of silently
  treating the broken scope as clean.
- **Bun global updates use `bun update -g`, never `bun upgrade`.** The latter
  upgrades the Bun runtime; package updates remain package-only.

### Changed
- **Bun now has candidate-version coverage** through its native, read-only
  `bun outdated -g` query. It is no longer listed as `update_only`.

### Added
- **`pum changelog [--json]`:** CLI-first embedded release notes with stable
  GitHub release, changelog, and raw-changelog links for agents and scripts.
- **`pum mcp`:** dependency-free MCP stdio server for local package freshness.
  It exposes read-only `pum_status`, `pum_update_plan`, and `pum_doctor`, plus
  `pum_refresh` which writes only PUM's local DuckDB snapshot. No MCP tool can
  mutate packages or the OS.

### Fixed
- **Brew cask upgrades no longer leave obsolete candidates in the DuckDB
  ledger.** Formula and cask inventory are now read independently; casks come
  from `brew info --cask --json=v2 --installed`. One untrusted third-party cask
  can therefore no longer hide every other cask and preserve a ghost
  “outdated” row after a successful upgrade.

### Changed
- Rebuilt the public README around the first useful minute: read-only current
  answer, review-before-mutation path, human/agent/repository recipes, explicit
  source coverage, and durable local-data boundaries.
- Added a human-centred social/README hero and preserved its exact image prompt
  and regeneration gate in `docs/assets/README.md`; the visible title now
  expands PUM to **Package Update Manager**.
- Pinned every GitHub Action to a reviewed commit SHA; cargo-dist and Rust
  bootstraps no longer pipe a remote script into a shell. Release credentials
  are limited to the publish job.

## [0.2.0] - 2026-07-23

### Added
- **DuckDB version ledger:** default inventory is now
  `~/.local/share/pum/inventory.duckdb`, with `refresh_runs` and append-only
  `version_observations` for historical latest-version queries.
- **`pum refresh [--json]`:** sequential, read-only `scan` + native-source
  check that writes one consistent inventory snapshot and JSON mirror.
- **`pum status [--json]`:** machine-readable freshness, latest candidates,
  DuckDB path, and explicit per-adapter source coverage for AI agents/CI.
- **`pum schedule --install`:** opt-in macOS launchd job at 09:05 by default;
  it invokes only `pum refresh --json`, never upgrades packages.

### Changed
- Existing default SQLite `inventory.db` imports once into DuckDB; the legacy
  file is left intact. An explicit `$PUM_DB` must now target a DuckDB file.
- The JSON mirror now serializes the persisted inventory so `scan` retains a
  prior check's `latest` and `status` values.
- Latest-version coverage is truthful: bun, uv, pipx, Go, and gh extension
  adapters are `update_only` until a safe non-mutating source resolver exists.

### Changed
- Refreshed README positioning around safe local package health checks, visible signals,
  and expected demo output.
- Replaced the social preview with a release-grade package-flow visual and recorded
  image provenance in `docs/assets/README.md`.
- Aligned the Cargo package description with the public safe-health-check positioning.

## [0.1.2] - 2026-07-02

### Fixed
- **`pnpm outdated` no longer misparses its own error output as a package.** When no
  global pnpm manifest exists, `pnpm outdated -g` prints an `ERR_PNPM_*` error line
  instead of a table; that line still had 3+ whitespace-separated tokens and was being
  read as a package named `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`.
- **`pum report --outdated` showed `unknown` as the installed version for every
  outdated Homebrew cask.** `installed_versions` in `brew outdated --json` is an array
  for casks too (same shape as formulae) — it was being read as a plain string, which
  always failed and fell back to `unknown`.

Both found while re-recording the README demo against this machine's real, messy
package inventory — a good reminder that a demo against real data is itself a QA step.

## [0.1.1] - 2026-07-02

### Fixed
- **`report --outdated` no longer shows ghost entries for already-upgraded packages.**
  The `(manager,name,installed)` primary key means an upgrade creates a *new* row
  instead of updating the old one; nothing ever pruned the stale old-version row, so
  packages already brought up to date kept showing as outdated forever. Every adapter
  except the two that legitimately keep multiple installed versions (mise, rustup) now
  prunes stale rows on `scan`.

### Added
- Prebuilt binaries (macOS arm64/x86_64, Linux arm64/x86_64, Windows x86_64) attached to
  every GitHub release via `cargo-dist`, plus a one-line shell installer — no Rust
  toolchain required to install `pum` anymore.
- Demo GIF, safety one-liner, and a pum-vs-topgrade/mise comparison table in the README.

## [0.1.0] - 2026-07-02

First public release. Single static Rust binary, 12 adapters, zero runtime deps.

### Added — project-local scanning + security audit (2026-06-06)
- **`pum project [path]`** — scans a repo's manifest for outdated **project** dependencies. The 12 global adapters only ever saw `-g` installs, so deps declared in a `package.json`/`Cargo.toml` were a blind spot (e.g. an app pinned to `zod@3` / `date-fns@3` showed nothing). Detects the ecosystem by lockfile and runs the native checker: bun (`bun outdated` table), pnpm/npm (`--json`), cargo (`cargo outdated --format json`). Verified live: found 10 outdated deps in a real Next.js project that `pum check` reported as clean.
- **`pum audit [path]`** — ghmax-style CVE/GHSA intel: queries the free **OSV.dev** batch API for known vulnerabilities affecting the exact installed versions (`bun pm ls` → OSV `querybatch` via `curl`, keeping pum a zero-HTTP-dep static binary). Verified live against OSV (clean project → no advisories; parse/index-mapping covered by unit test).
- New modules `src/project.rs` + `src/audit.rs` (pure parse fns + 6 unit tests); both wrapped so a missing/broken project toolchain never aborts pum. `run::run_in` added for cwd-scoped subprocess calls.
- Gates: `cargo fmt --check` clean · `cargo clippy --all-targets -D warnings` clean · 13 tests pass.

### Added — project/audit enrichment + JSON (2026-06-06)
- **`pum audit` now shows severity + fix version** — each advisory is enriched from the OSV detail endpoint (`/v1/vulns/<id>`): severity label (e.g. `HIGH`) and the version(s) that fix it. `parse_osv_detail` is a pure fn with a unit test.
- **`pum project` flags deprecated packages** — outdated node deps are checked against the npm registry (`npm view <pkg>@<ver> deprecated`, run in parallel via rayon); deprecated ones are marked `[deprecated]` and counted.
- **`--json` on `pum project` and `pum audit`** — machine-readable output for CI gates (serde-serialized `ProjectDep` / `Vuln`).
- Verified live: `pum project --json` emits the dependency array; `pum audit` runs the OSV detail enrichment. 14 unit tests pass; clippy + fmt clean.

### Fixed — lint + reliability hardening (2026-06-03)
- **clippy now actually clean** (`cargo clippy --all-targets`, 0 warnings). The prior "clippy clean" claim had drifted: 5 lints had crept in. Fixed: `sort_by` → `sort_by_key(Reverse)`, collapsible-`if` ×2, manual-char-compare, and `enum Cmd` variant `SelfCmd` → `SelfUpdate` (CLI name still pinned to `self` via `#[command(name = "self")]`, verified).
- **run.rs hardening:** replaced a guarded `split_first().unwrap()` with `let-else` (dedups the empty-argv check, removes the unwrap); documented the intentional `tx.send` error-ignore. Debugmaster audit: Grade A (97/100), Release SHIP, 0 critical/high.
- **README:** Requirements footer corrected from stale Python (`Python 3.11+ / stdlib / ruff`) to Rust (`Rust 1.85+ edition 2024, cargo build/clippy/test`).

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
