# PUM release contract

Do not publish merely because a local binary works. A PUM release changes a
machine-maintenance tool and its embedded database engine.

## Pre-tag gate

From the repository root:

```bash
just ci
cd apps/pum && cargo build --release
PUM_DB="$(mktemp -d)/inventory.duckdb" target/release/pum refresh --json
PUM_DB="$(mktemp -d)/inventory.duckdb" target/release/pum status --json
```

Confirm all of these manually before the tag:

1. `CHANGELOG.md` has an exact version/date and calls out storage migration.
2. `Cargo.toml` and `Cargo.lock` pin the reviewed DuckDB binding version.
3. A legacy `~/.local/share/pum/inventory.db` imports into a new
   `inventory.duckdb`; the source SQLite file remains untouched.
4. `refresh --json` has a timestamp, inventory count, latest candidates, and
   no false `current` claim for an `update_only` adapter.
5. `schedule --install` was smoke-tested once on macOS and its plist only calls
   `refresh --json`, never `update` or `self --apply`.
6. Every GitHub Action is pinned to a reviewed full commit SHA, and the
   cargo-dist bootstrap is a checksum/signature-verified download, never
   `curl | sh`.

## Publish

1. Push the reviewed commit to `main`; wait for CI and cargo-deny to pass.
2. Create signed annotated tag `vX.Y.Z` only from that commit and push it.
3. The release workflow first runs format, clippy, tests, release build, and
   cargo-deny; only then does cargo-dist build the five configured targets,
   archive hashes, shell installer, and GitHub release assets. Preserve that
   `verify` job if cargo-dist regenerates the workflow.
4. Download each archive from the immutable release URL, verify its `.sha256`,
   then run `pum --version`, `pum doctor`, and a temporary-DB `pum refresh` on
   its native platform.
5. Test `pum-installer.sh` against that immutable tag URL. Only then update the
   Homebrew tap formula and announce the release.

## Do not do

- Do not make PUM self-update automatically until release metadata and every
  downloaded binary are signature-verified.
- Do not label unsupported adapter sources current because their command printed
  no candidates.
- Do not delete a user's legacy SQLite inventory after import.
