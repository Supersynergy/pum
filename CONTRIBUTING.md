# Contributing

`pum` is a Rust-first, single-binary package/tool update manager. Contributions
should keep it a static binary with zero runtime deps.

## Local Checks

Run the full gate before sending changes (from `apps/pum/`):

```bash
cargo test
cargo clippy --all-targets -- -D warnings && cargo fmt --check
cargo run -q -- doctor
```

## Adapter Changes

- New manager → new file in `src/adapters/<name>.rs`, implement the `Adapter`
  trait, register in `all_adapters()` (`src/adapters/mod.rs`).
- Keep parse logic in a pure `pub fn` with a unit test in `main.rs`.
- Every subprocess call goes through `run::run(argv, timeout)` — never
  `std::process::Command` directly, never hang, never panic.
- Never trigger updates from `scan` or `check`.
- Packages & developer tools only — never add an OS updater (see `AGENTS.md`).

## Pull Requests

- Keep each PR focused on one adapter or one behavior.
- Update `CHANGELOG.md` for user-visible commands or adapter changes.
- Include the verification commands you ran (and their output where relevant).
