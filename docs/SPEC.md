# pum — Package Update Manager Spec

## Overview

`pum` is a CLI tool that maintains a unified inventory of packages installed by multiple package managers on a single machine. It follows an adapter registry pattern: each manager is a class implementing a fixed protocol; only adapters whose binary exists on PATH are activated.

## Adapter Contract

Every adapter implements the `Adapter` trait (`src/adapters/mod.rs`):

```rust
pub trait Adapter: Send + Sync {
    fn name(&self) -> &str;             // unique short id (e.g. "brew")
    fn binary(&self) -> &str;           // binary probed via which::which()
    fn detect(&self) -> bool;           // default: which(binary).is_ok()
    fn list_installed(&self) -> Vec<Package>;
    fn list_outdated(&self) -> Vec<Package>;          // status="outdated"
    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String>;  // argv, never executed here
    fn self_update_cmd(&self) -> Vec<String> { vec![] }
    fn report_only(&self) -> bool { false }           // excluded from `update --all`
}
```

Rules:
- Every subprocess call uses `run::run()` with a timeout (default 60s); it never panics on missing binary or non-zero exit.
- Adapters return `Vec<Package>` and never panic; the scan wraps each in `catch_unwind` so one failing adapter cannot abort the run.
- `list_outdated()` is separate from `list_installed()` and is called only by
  the read-only `pum check` and `pum refresh` paths.
- Updates are NEVER triggered during `scan`, `check`, or `refresh`.

## Data Model

DuckDB table `tools` at `~/.local/share/pum/inventory.duckdb` (override with `$PUM_DB`):

| Column      | Type | Notes |
|-------------|------|-------|
| manager     | TEXT | adapter name |
| name        | TEXT | package name |
| installed   | TEXT | installed version string |
| latest      | TEXT | latest known version |
| status      | TEXT | current \| outdated \| unknown |
| source      | TEXT | e.g. brew, brew-cask, npm-global |
| checked_at  | TEXT | ISO-8601 UTC timestamp |

Primary key: `(manager, name, installed)` — multiple versions of one name (e.g. mise
`python` 3.12/3.13/3.14) each persist. Upserts on scan/check; a re-`scan` never wipes a
prior `check` result (latest/status only overwritten when the incoming row has a real value).

`refresh_runs` records completed read-only refreshes; `version_observations`
contains the append-only package snapshot for each run. The current inventory
is mirrored to `inventory.json` (same dir) on every `pum scan` and `pum refresh`.
The former SQLite `inventory.db` is imported once at the default path and then
left untouched.

## Commands

| Command | Description | Writes DB | Runs Updates |
|---------|-------------|-----------|--------------|
| `pum doctor` | List adapters and their detection status | no | no |
| `pum scan` | Inventory all installed packages | yes | no |
| `pum check` | Query outdated packages | yes | no |
| `pum refresh [--json]` | Sequential scan + source check + DuckDB snapshot | yes | no |
| `pum status [--json]` | Freshness, candidates, and source coverage | no | no |
| `pum report [--json] [--outdated] [--manager M]` | Print table from DB | no | no |
| `pum update [--manager M\|--all] [--dry-run] [pkg...]` | Upgrade packages | no | yes |
| `pum schedule --install\|--remove` | Install/remove daily macOS `refresh` | no | launchd only |
| `pum self [--apply]` | Check/run manager self-updates | no | with --apply |
| `pum mcp` | MCP stdio server: status, refresh, doctor, update plan | local ledger only | no |

## MCP interface

`pum mcp` implements the MCP stdio transport: newline-delimited JSON-RPC on
stdin/stdout, with diagnostics only on stderr. It exposes `pum_status`,
`pum_refresh`, `pum_update_plan`, and `pum_doctor`. `pum_refresh` can contact
native package-manager sources and append a local DuckDB snapshot; it does not
install, remove, upgrade, or update the operating system. The server offers no
package-mutation tool, so a human must still invoke the reviewed CLI update.

## Adapters Implemented

| Adapter | Binary | Installed | Outdated | Self-update |
|---------|--------|-----------|----------|-------------|
| brew | brew | `brew list --versions` | `brew outdated --json=v2` | `brew update` |
| npm | npm | `npm ls -g --depth=0 --json` | `npm outdated -g --json` | via brew |
| pnpm | pnpm | `pnpm ls -g` | `pnpm outdated -g` | — |
| bun | bun | `bun pm ls -g` | — | `bun upgrade` |
| uv | uv | `uv tool list` | — | `uv self update` |
| pipx | pipx | `pipx list --json` | — | — |
| cargo | cargo | `cargo install --list` | `cargo install-update -l` | — |
| rustup | rustup | `rustup toolchain list` | `rustup check` | `rustup self update` |
| gem | gem | `gem list` | `gem outdated` | — |
| go | go | `ls $(go env GOPATH)/bin` | — | — |
| mise | mise | `mise ls --current` | `mise outdated` | — |
| gh | gh | `gh extension list` | — | — |

## Scope — packages & tools only, never the OS

pum manages package managers and developer tools only. It deliberately does **not** include
the macOS `softwareupdate` (OS) updater: scanning or installing OS updates can trigger
reboots and is too dangerous to automate. There is no adapter for it.

## Error Handling

- Per-adapter errors are caught, logged to stderr, and do not abort the overall scan.
- Go binary versions are not available from the binary; `installed` is set to `"unknown"`.
- `report_only` adapters (none by default) are excluded from `pum update --all` and require
  an explicit `pum update --manager <name> --apply`.

## Concurrency

`scan` and `check` run adapters concurrently via `rayon` (`par_iter`), each wrapped in
`catch_unwind`. `refresh` executes its scan phase before its check phase so one
DuckDB snapshot is persisted by one writer. `update` runs sequentially per adapter to avoid
conflicting package-manager locks.
