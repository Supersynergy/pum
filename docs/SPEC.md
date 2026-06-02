# pum — Package Update Manager Spec

## Overview

`pum` is a CLI tool that maintains a unified inventory of packages installed by multiple package managers on a single machine. It follows an adapter registry pattern: each manager is a class implementing a fixed protocol; only adapters whose binary exists on PATH are activated.

## Adapter Contract

Every adapter must subclass `Adapter` and implement:

```python
class Adapter:
    name: str          # unique short id (e.g. "brew", "cargo")
    binary: str        # binary to probe with shutil.which()

    def detect(self) -> bool:
        """Return True if this manager is present on the host."""

    def list_installed(self) -> list[Package]:
        """Return all installed packages with name + installed version."""

    def list_outdated(self) -> list[Package]:
        """Return packages with installed < latest; set status='outdated'."""

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        """Return argv to upgrade pkg (or all if pkg=None). Never execute."""

    def self_update_cmd(self) -> list[str]:
        """Return argv to update the manager itself. Never execute."""
```

Rules:
- Every subprocess call must use `_run()` with a timeout (default 60s).
- Adapters must never raise; catch all exceptions internally and return `[]`.
- `list_outdated()` is separate from `list_installed()` and is ONLY called by `pum check`.
- Updates are NEVER triggered during `scan` or `check`.

## Data Model

SQLite table `tools` at `data/inventory.db`:

| Column      | Type | Notes |
|-------------|------|-------|
| manager     | TEXT | adapter name |
| name        | TEXT | package name |
| installed   | TEXT | installed version string |
| latest      | TEXT | latest known version |
| status      | TEXT | current \| outdated \| unknown |
| source      | TEXT | e.g. brew, brew-cask, npm-global |
| checked_at  | TEXT | ISO-8601 UTC timestamp |

Primary key: `(manager, name)`. Upserts on every scan/check.

Mirrored to `data/inventory.json` on every `pum scan`.

## Commands

| Command | Description | Writes DB | Runs Updates |
|---------|-------------|-----------|--------------|
| `pum doctor` | List adapters and their detection status | no | no |
| `pum scan` | Inventory all installed packages | yes | no |
| `pum check` | Query outdated packages | yes | no |
| `pum report [--json] [--outdated] [--manager M]` | Print table from DB | no | no |
| `pum update [--manager M\|--all] [--dry-run] [pkg...]` | Upgrade packages | no | yes |
| `pum self [--apply]` | Check/run manager self-updates | no | with --apply |

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

`scan` and `check` run adapters concurrently using `concurrent.futures.ThreadPoolExecutor(max_workers=8)`.
`update` runs sequentially per adapter to avoid conflicting package manager locks.
