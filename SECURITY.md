# Security Policy

## Reporting

Report security issues privately to `true@supersynergy.de`.

Please include:

- affected version or commit
- reproduction steps
- expected vs. observed behavior
- impact assessment

## Scope

Security-sensitive areas include:

- command construction/execution for each adapter (`src/adapters/*.rs`, run via
  `run::run(argv, timeout)`) — argv must never be built from unsanitized input
- the OSV.dev audit path (`src/audit.rs`) — response parsing must not trust
  remote data beyond the documented schema
- inventory DB writes (`~/.local/share/pum/inventory.duckdb`) — no SQL built from
  unsanitized strings
- DuckDB opens with external file/network access and extension autoload disabled;
  PUM uses only parameterized local inventory queries
- panics reachable from a malformed manager output (adapters must degrade,
  never crash the whole scan)

## Safe Defaults

`pum` never triggers package installs or upgrades from `scan`, `check`, `refresh`,
`status`, `report`, `project`, or `audit` — only `update`/`self --apply` mutate packages,
and only for the manager(s) explicitly named. `schedule --install` only writes an
opt-in launchd job that invokes the read-only `refresh` command. `pum` never manages the
operating system itself (no `softwareupdate` adapter, by design — see
`AGENTS.md`).
