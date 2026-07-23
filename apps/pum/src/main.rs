//! pum — Package Update Manager (Rust).
//! Mac-first CLI: scan every package manager + dev tool, inventory versions, check
//! updates, update per-tool or all. Packages & tools only — never the OS.
mod adapters;
mod audit;
mod mcp;
mod project;
mod run;
mod types;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, Subcommand};
use duckdb::{Config, Connection, params};
use rusqlite::Connection as LegacyConnection;
use serde::Serialize;

use adapters::{Adapter, get_adapter, live_adapters};
use types::Package;

// ── colors ──────────────────────────────────────────────────────────────────
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

fn c(text: &str, code: &str) -> String {
    if std::io::stdout().is_terminal() {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

// ── paths ─────────────────────────────────────────────────────────────────--
fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("PUM_DB") {
        return PathBuf::from(p)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share")
        });
    base.join("pum")
}

fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("PUM_DB") {
        return PathBuf::from(p);
    }
    data_dir().join("inventory.duckdb")
}

/// Pre-0.2 inventories were SQLite. The default path is kept solely so a
/// user's local history can be imported once into the DuckDB default.
fn legacy_db_path() -> PathBuf {
    data_dir().join("inventory.db")
}

fn json_path() -> PathBuf {
    data_dir().join("inventory.json")
}

// ── db ────────────────────────────────────────────────────────────────────--
fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tools (
            manager TEXT NOT NULL,
            name TEXT NOT NULL,
            installed TEXT NOT NULL DEFAULT '',
            latest TEXT,
            status TEXT,
            source TEXT,
            checked_at TEXT,
            PRIMARY KEY (manager, name, installed)
         );
         CREATE TABLE IF NOT EXISTS refresh_runs (
            run_id TEXT PRIMARY KEY,
            started_at TEXT NOT NULL,
            completed_at TEXT NOT NULL,
            package_count BIGINT NOT NULL,
            update_count BIGINT NOT NULL,
            adapter_errors TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS version_observations (
            run_id TEXT NOT NULL,
            observed_at TEXT NOT NULL,
            manager TEXT NOT NULL,
            name TEXT NOT NULL,
            installed TEXT NOT NULL,
            latest TEXT,
            status TEXT,
            source TEXT
         );
         CREATE TABLE IF NOT EXISTS pum_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );",
    )?;
    Ok(())
}

fn is_sqlite_db(path: &std::path::Path) -> bool {
    std::fs::read(path)
        .ok()
        .is_some_and(|bytes| bytes.starts_with(b"SQLite format 3\0"))
}

/// Import the original inventory only for the normal default path. An explicit
/// PUM_DB is an operator-owned contract, so pointing it at SQLite returns an
/// actionable error instead of silently changing its filename or contents.
fn migrate_default_legacy_sqlite(conn: &Connection) -> Result<usize> {
    let legacy_path = legacy_db_path();
    if !legacy_path.is_file() {
        return Ok(0);
    }
    let legacy = LegacyConnection::open(&legacy_path)?;
    let mut stmt = match legacy
        .prepare("SELECT manager,name,installed,latest,status,source,checked_at FROM tools")
    {
        Ok(stmt) => stmt,
        Err(_) => return Ok(0),
    };
    let rows = stmt.query_map([], |r| {
        Ok(Package {
            manager: r.get(0)?,
            name: r.get(1)?,
            installed: r.get(2)?,
            latest: r.get(3)?,
            status: r.get(4)?,
            source: r.get(5)?,
            checked_at: r.get(6).unwrap_or_default(),
        })
    })?;
    let pkgs: Vec<Package> = rows.filter_map(|r| r.ok()).collect();
    upsert(conn, &pkgs)?;
    conn.execute(
        "INSERT INTO pum_meta (key,value) VALUES ('legacy_sqlite_import', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![legacy_path.display().to_string()],
    )?;
    Ok(pkgs.len())
}

fn db_connect() -> Result<Connection> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let path = db_path();
    if std::env::var_os("PUM_DB").is_some() && path.is_file() && is_sqlite_db(&path) {
        bail!(
            "PUM_DB points at a legacy SQLite file ({}). Set PUM_DB to a .duckdb path, then run pum refresh.",
            path.display()
        );
    }
    let created = !path.exists();
    // PUM uses only parameterized local tables. Disable DuckDB external access
    // and extension autoload so inventory data can never trigger a network/file
    // read or an extension install through a future query change.
    let config = Config::default()
        .enable_external_access(false)?
        .enable_autoload_extension(false)?;
    let conn = Connection::open_with_flags(&path, config)?;
    init_schema(&conn)?;
    if created && std::env::var_os("PUM_DB").is_none() {
        let imported = migrate_default_legacy_sqlite(&conn)?;
        if imported > 0 {
            eprintln!(
                "pum: imported {imported} legacy SQLite inventory rows into {}",
                path.display()
            );
        }
    }
    Ok(conn)
}

/// Upsert that NEVER wipes a prior `check` result on a plain re-`scan`: latest/status are
/// only overwritten when the incoming row actually carries a real value.
fn upsert(conn: &Connection, pkgs: &[Package]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO tools (manager,name,installed,latest,status,source,checked_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(manager,name,installed) DO UPDATE SET
                latest=CASE WHEN excluded.latest IS NULL OR excluded.latest=''
                            THEN tools.latest ELSE excluded.latest END,
                status=CASE WHEN excluded.status='unknown' OR excluded.status IS NULL
                            THEN tools.status ELSE excluded.status END,
                source=excluded.source,
                checked_at=excluded.checked_at",
        )?;
        for p in pkgs {
            stmt.execute(params![
                p.manager,
                p.name,
                p.installed,
                p.latest,
                p.status,
                p.source,
                p.checked_at,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Delete stale (manager,name,old_installed) rows once a fresh scan shows the
/// package now installed at a different version — for every adapter except
/// the multi-version ones (mise, rustup), an install *replaces* the prior
/// version, so the old row is a ghost that would otherwise linger forever and
/// keep showing up as "outdated" in `report` even after a real upgrade.
fn prune_stale(conn: &Connection, pkgs: &[Package]) -> Result<usize> {
    use std::collections::{HashMap, HashSet};
    let multi_version: HashSet<String> = adapters::all_adapters()
        .into_iter()
        .filter(|a| a.multi_version())
        .map(|a| a.name().to_string())
        .collect();

    let mut current: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for p in pkgs {
        if multi_version.contains(&p.manager) {
            continue;
        }
        current
            .entry((p.manager.clone(), p.name.clone()))
            .or_default()
            .insert(p.installed.clone());
    }

    let mut pruned = 0;
    for ((manager, name), installed_versions) in &current {
        let placeholders = installed_versions
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM tools WHERE manager = ? AND name = ? AND installed NOT IN ({placeholders})"
        );
        let mut params: Vec<&dyn duckdb::ToSql> = vec![manager, name];
        for v in installed_versions {
            params.push(v);
        }
        pruned += conn.execute(&sql, params.as_slice())?;
    }

    // `brew outdated --json` identifies a tapped formula as `tap/name`, while
    // `brew list` uses its installed token `name`. Remove the legacy alias as
    // soon as the canonical token was observed, or it will remain an outdated
    // ghost even after a successful upgrade.
    let brew_tokens: HashSet<&str> = current
        .keys()
        .filter(|(manager, _)| manager == "brew")
        .map(|(_, name)| name.as_str())
        .collect();
    for old in load_all(conn)? {
        if old.manager == "brew"
            && old.name.contains('/')
            && old
                .name
                .rsplit('/')
                .next()
                .is_some_and(|token| brew_tokens.contains(token))
        {
            pruned += conn.execute(
                "DELETE FROM tools WHERE manager = ?1 AND name = ?2 AND installed = ?3",
                params![old.manager, old.name, old.installed],
            )?;
        }
    }
    Ok(pruned)
}

fn load_all(conn: &Connection) -> Result<Vec<Package>> {
    let mut stmt =
        conn.prepare("SELECT manager,name,installed,latest,status,source,checked_at FROM tools")?;
    let rows = stmt.query_map([], |r| {
        Ok(Package {
            manager: r.get(0)?,
            name: r.get(1)?,
            installed: r.get(2)?,
            latest: r.get(3)?,
            status: r.get(4)?,
            source: r.get(5)?,
            checked_at: r.get(6).unwrap_or_default(),
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn write_json(pkgs: &[Package]) -> Result<()> {
    std::fs::write(json_path(), serde_json::to_string_pretty(pkgs)?)?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct SourceCoverage {
    manager: String,
    source: &'static str,
    mode: &'static str,
}

#[derive(Debug, Serialize)]
struct RefreshSummary {
    database: String,
    started_at: String,
    completed_at: String,
    package_count: usize,
    update_count: usize,
    adapter_errors: Vec<String>,
    source_coverage: Vec<SourceCoverage>,
}

#[derive(Debug, Serialize)]
struct LastRefresh {
    completed_at: String,
    package_count: i64,
    update_count: i64,
    adapter_errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusSummary {
    database: String,
    schema: &'static str,
    inventory_count: usize,
    outdated_count: usize,
    last_refresh: Option<LastRefresh>,
    stale: bool,
    latest_candidates: Vec<Package>,
    source_coverage: Vec<SourceCoverage>,
}

/// The native manager commands below are the source of truth. A manager with
/// `update_only` is deliberately *not* reported as current: it has a safe
/// updater but no non-mutating latest-version query in PUM yet.
fn source_coverage() -> Vec<SourceCoverage> {
    live_adapters()
        .into_iter()
        .map(|adapter| {
            let (source, mode) = match adapter.name() {
                "brew" => (
                    "Homebrew API via brew outdated --json=v2",
                    "candidate_versions",
                ),
                "npm" => (
                    "npm registry via npm outdated -g --json",
                    "candidate_versions",
                ),
                "pnpm" => ("npm registry via pnpm outdated -g", "candidate_versions"),
                "cargo" => (
                    "crates.io via cargo-install-update -l",
                    "candidate_versions",
                ),
                "gem" => ("RubyGems via gem outdated", "candidate_versions"),
                "mise" => ("mise registry via mise outdated", "candidate_versions"),
                "rustup" => ("Rust distribution via rustup check", "candidate_versions"),
                "bun" => ("no non-mutating global outdated query wired", "update_only"),
                "uv" => (
                    "no non-mutating uv tool outdated query wired",
                    "update_only",
                ),
                "pipx" => ("no non-mutating pipx outdated query wired", "update_only"),
                "go" => (
                    "installed Go binaries lack reliable provenance",
                    "update_only",
                ),
                "gh" => (
                    "no non-mutating gh extension outdated query wired",
                    "update_only",
                ),
                _ => ("unknown", "unknown"),
            };
            SourceCoverage {
                manager: adapter.name().to_string(),
                source,
                mode,
            }
        })
        .collect()
}

fn record_refresh(
    conn: &Connection,
    started_at: &str,
    pkgs: &[Package],
    update_count: usize,
    errors: &[String],
) -> Result<RefreshSummary> {
    let completed_at = Utc::now().to_rfc3339();
    let run_id = format!("{}-{}", completed_at, std::process::id());
    let errors_json = serde_json::to_string(errors)?;
    conn.execute(
        "INSERT INTO refresh_runs
         (run_id,started_at,completed_at,package_count,update_count,adapter_errors)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            run_id,
            started_at,
            completed_at,
            pkgs.len() as i64,
            update_count as i64,
            errors_json
        ],
    )?;
    let mut statement = conn.prepare(
        "INSERT INTO version_observations
         (run_id,observed_at,manager,name,installed,latest,status,source)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
    )?;
    for pkg in pkgs {
        statement.execute(params![
            run_id,
            completed_at,
            pkg.manager,
            pkg.name,
            pkg.installed,
            pkg.latest,
            pkg.status,
            pkg.source,
        ])?;
    }
    Ok(RefreshSummary {
        database: db_path().display().to_string(),
        started_at: started_at.to_string(),
        completed_at,
        package_count: pkgs.len(),
        update_count,
        adapter_errors: errors.to_vec(),
        source_coverage: source_coverage(),
    })
}

fn refresh_inventory() -> Result<RefreshSummary> {
    let started_at = Utc::now().to_rfc3339();
    // Deliberately sequential: DuckDB has a single-writer model and a refresh
    // must snapshot one scan followed by one source check, never two writers.
    let (installed, scan_errors) = collect(|adapter| adapter.list_installed());
    let (updates, check_errors) = collect(|adapter| adapter.list_outdated());
    let mut errors = scan_errors;
    errors.extend(check_errors);

    let conn = db_connect()?;
    upsert(&conn, &installed)?;
    prune_stale(&conn, &installed)?;
    upsert(&conn, &updates)?;
    let all = load_all(&conn)?;
    write_json(&all)?;
    let update_count = all
        .iter()
        .filter(|pkg| pkg.status.as_deref() == Some("outdated"))
        .count();
    record_refresh(&conn, &started_at, &all, update_count, &errors)
}

fn load_status(conn: &Connection) -> Result<StatusSummary> {
    let all = load_all(conn)?;
    let latest_candidates: Vec<Package> = all
        .iter()
        .filter(|pkg| pkg.status.as_deref() == Some("outdated"))
        .cloned()
        .collect();
    let last_refresh = conn
        .query_row(
            "SELECT completed_at,package_count,update_count,adapter_errors
             FROM refresh_runs ORDER BY completed_at DESC LIMIT 1",
            [],
            |row| {
                let adapter_errors: String = row.get(3)?;
                Ok(LastRefresh {
                    completed_at: row.get(0)?,
                    package_count: row.get(1)?,
                    update_count: row.get(2)?,
                    adapter_errors: serde_json::from_str(&adapter_errors).unwrap_or_default(),
                })
            },
        )
        .ok();
    let stale = last_refresh
        .as_ref()
        .and_then(|run| DateTime::parse_from_rfc3339(&run.completed_at).ok())
        .is_none_or(|checked_at| {
            Utc::now().signed_duration_since(checked_at.with_timezone(&Utc)) > Duration::hours(26)
        });
    Ok(StatusSummary {
        database: db_path().display().to_string(),
        schema: "duckdb-v1",
        inventory_count: all.len(),
        outdated_count: latest_candidates.len(),
        last_refresh,
        stale,
        latest_candidates,
        source_coverage: source_coverage(),
    })
}

// ── parallel collection ───────────────────────────────────────────────────--
fn collect<F>(f: F) -> (Vec<Package>, Vec<String>)
where
    F: Fn(&dyn Adapter) -> Vec<Package> + Sync,
{
    use rayon::prelude::*;
    let adapters = live_adapters();
    let results: Vec<(Vec<Package>, Option<String>)> = adapters
        .par_iter()
        .map(|a| {
            // an adapter erroring must never abort the run
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(a.as_ref()))) {
                Ok(pkgs) => (pkgs, None),
                Err(_) => (vec![], Some(format!("{}: adapter panicked", a.name()))),
            }
        })
        .collect();
    let mut pkgs = Vec::new();
    let mut errs = Vec::new();
    for (p, e) in results {
        pkgs.extend(p);
        if let Some(e) = e {
            errs.push(e);
        }
    }
    (pkgs, errs)
}

// ── commands ──────────────────────────────────────────────────────────────--
fn cmd_doctor() -> Result<()> {
    println!("\n{} — live adapters\n", c("pum doctor", BOLD));
    for a in adapters::all_adapters() {
        let live = a.detect();
        let mark = if live {
            c("live", GREEN)
        } else {
            c("—", DIM)
        };
        let path = which::which(a.binary())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        println!("  {:<6} {:<12} {}", mark, a.name(), c(&path, DIM));
    }
    println!(
        "\n  {} packages & developer tools only — the OS updater is excluded by design.\n",
        c("note:", DIM)
    );
    Ok(())
}

fn cmd_scan() -> Result<()> {
    let (pkgs, errs) = collect(|a| a.list_installed());
    let conn = db_connect()?;
    upsert(&conn, &pkgs)?;
    prune_stale(&conn, &pkgs)?;
    // The JSON mirror must carry a prior check's latest/status values too,
    // not only the raw rows returned by this scan.
    write_json(&load_all(&conn)?)?;

    let mut by_mgr: std::collections::BTreeMap<String, usize> = Default::default();
    for p in &pkgs {
        *by_mgr.entry(p.manager.clone()).or_default() += 1;
    }
    println!("\n{} — {} packages\n", c("pum scan", BOLD), pkgs.len());
    let mut sorted: Vec<_> = by_mgr.into_iter().collect();
    sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
    let line: Vec<String> = sorted.iter().map(|(m, n)| format!("{m} {n}")).collect();
    println!("  {}", line.join(" · "));
    if !errs.is_empty() {
        println!("\n  {} {}", c("adapter errors:", YELLOW), errs.join("; "));
    }
    println!(
        "\n  db→ {}\n  json→ {}\n",
        db_path().display(),
        json_path().display()
    );
    Ok(())
}

fn cmd_check() -> Result<()> {
    let (pkgs, errs) = collect(|a| a.list_outdated());
    let conn = db_connect()?;
    upsert(&conn, &pkgs)?;
    let n = pkgs
        .iter()
        .filter(|p| p.status.as_deref() == Some("outdated"))
        .count();
    println!(
        "\n{} — {} updates available",
        c("pum check", BOLD),
        c(&n.to_string(), if n > 0 { YELLOW } else { GREEN })
    );
    if !errs.is_empty() {
        println!("  {} {}", c("adapter errors:", YELLOW), errs.join("; "));
    }
    println!();
    Ok(())
}

fn cmd_refresh(json: bool) -> Result<()> {
    let summary = refresh_inventory()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }
    println!(
        "\n{} — {} packages, {} updates\n",
        c("pum refresh", BOLD),
        summary.package_count,
        c(
            &summary.update_count.to_string(),
            if summary.update_count > 0 {
                YELLOW
            } else {
                GREEN
            }
        )
    );
    for coverage in &summary.source_coverage {
        let mode = if coverage.mode == "candidate_versions" {
            c("latest", GREEN)
        } else {
            c("needs source", YELLOW)
        };
        println!("  {:<7} {:<14} {}", coverage.manager, mode, coverage.source);
    }
    if !summary.adapter_errors.is_empty() {
        println!(
            "\n  {} {}",
            c("adapter errors:", YELLOW),
            summary.adapter_errors.join("; ")
        );
    }
    println!("\n  db→ {}\n", summary.database);
    Ok(())
}

fn cmd_status(json: bool) -> Result<()> {
    let conn = db_connect()?;
    let status = load_status(&conn)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }
    let freshness = if status.stale {
        c("stale — run: pum refresh", YELLOW)
    } else {
        c("fresh", GREEN)
    };
    println!(
        "\n{} — {} packages, {} updates, {}\n",
        c("pum status", BOLD),
        status.inventory_count,
        c(
            &status.outdated_count.to_string(),
            if status.outdated_count > 0 {
                YELLOW
            } else {
                GREEN
            }
        ),
        freshness
    );
    match &status.last_refresh {
        Some(run) => println!("  last refresh: {}", run.completed_at),
        None => println!("  last refresh: never"),
    }
    for coverage in &status.source_coverage {
        let mode = if coverage.mode == "candidate_versions" {
            c("latest", GREEN)
        } else {
            c("needs source", YELLOW)
        };
        println!("  {:<7} {:<14} {}", coverage.manager, mode, coverage.source);
    }
    println!("\n  db→ {}\n", status.database);
    Ok(())
}

fn cmd_report(json: bool, outdated: bool, manager: Option<&str>) -> Result<()> {
    let conn = db_connect()?;
    let all = load_all(&conn)?;
    let total = all.len();
    let pkgs: Vec<&Package> = all
        .iter()
        .filter(|p| !outdated || p.status.as_deref() == Some("outdated"))
        .filter(|p| manager.is_none_or(|m| p.manager == m))
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&pkgs)?);
        return Ok(());
    }
    if pkgs.is_empty() {
        if total == 0 {
            println!("No packages in inventory. Run: pum scan");
        } else {
            let mut filt = Vec::new();
            if outdated {
                filt.push("--outdated".to_string());
            }
            if let Some(m) = manager {
                filt.push(format!("--manager {m}"));
            }
            let f = if filt.is_empty() {
                "the filter".into()
            } else {
                filt.join(" ")
            };
            println!("No packages match {f} ({total} in inventory).");
        }
        return Ok(());
    }

    let w_mgr = pkgs
        .iter()
        .map(|p| p.manager.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let w_pkg = pkgs.iter().map(|p| p.name.len()).max().unwrap_or(7).max(7);
    let w_inst = pkgs
        .iter()
        .map(|p| p.installed.len())
        .max()
        .unwrap_or(9)
        .max(9);
    let w_lat = pkgs
        .iter()
        .map(|p| p.latest.as_deref().unwrap_or("").len())
        .max()
        .unwrap_or(6)
        .max(6);
    println!(
        "\n{}",
        c(
            &format!(
                "  {:<w_mgr$}  {:<w_pkg$}  {:<w_inst$}  {:<w_lat$}  STATUS",
                "MANAGER", "PACKAGE", "INSTALLED", "LATEST"
            ),
            BOLD
        )
    );
    for p in &pkgs {
        let status = p.status.as_deref().unwrap_or("unknown");
        let (mark, scol) = match status {
            "outdated" => ("!", YELLOW),
            "current" => (" ", GREEN),
            _ => (" ", DIM),
        };
        println!(
            "{} {:<w_mgr$}  {:<w_pkg$}  {:<w_inst$}  {:<w_lat$}  {}",
            mark,
            p.manager,
            p.name,
            p.installed,
            p.latest.as_deref().unwrap_or("—"),
            c(status, scol),
        );
    }
    println!();
    Ok(())
}

fn cmd_update(
    packages: &[String],
    manager: Option<&str>,
    all: bool,
    dry_run: bool,
    apply: bool,
) -> Result<()> {
    println!("\n{}\n", c("pum update", BOLD));

    // specific packages → find owning manager from inventory
    if !packages.is_empty() {
        let conn = db_connect()?;
        let mut applied = false;
        for pkg in packages {
            let mgr: Option<String> = conn
                .query_row(
                    "SELECT manager FROM tools WHERE name=?1 ORDER BY checked_at DESC LIMIT 1",
                    params![pkg],
                    |r| r.get(0),
                )
                .ok();
            let Some(mgr) = mgr else {
                println!("  {pkg}: not found in inventory (run pum scan)");
                continue;
            };
            let Some(adapter) = get_adapter(&mgr) else {
                println!("  {pkg}: adapter '{mgr}' not in registry");
                continue;
            };
            applied |= run_one(adapter.as_ref(), Some(pkg), dry_run);
        }
        if applied {
            println!("\n  refreshing version ledger after successful update …");
            cmd_refresh(false)?;
        }
        return Ok(());
    }

    // single manager
    let adapters = if let Some(m) = manager {
        match get_adapter(m) {
            Some(a) if a.detect() => vec![a],
            _ => {
                println!("Adapter '{m}' not found or not installed.");
                std::process::exit(1);
            }
        }
    } else {
        live_adapters()
    };

    // bulk: delegate to topgrade when present
    if all && which::which("topgrade").is_ok() {
        if dry_run {
            println!("  [dry-run] topgrade");
        } else {
            println!("  running topgrade …");
            let (rc, out, err) = run::run(&["topgrade"], 600);
            print!("{out}");
            if rc != 0 {
                println!("{} {err}", c("topgrade error:", RED));
            } else {
                println!("\n  refreshing version ledger after successful update …");
                cmd_refresh(false)?;
            }
        }
        return Ok(());
    }

    let mut applied = false;
    for a in &adapters {
        if a.report_only() {
            let targeted = manager == Some(a.name());
            if !(targeted && apply) {
                println!(
                    "  [{}] {}. Run: pum update --manager {} --apply",
                    a.name(),
                    c("skipped — report-only", YELLOW),
                    a.name()
                );
                continue;
            }
        }
        applied |= run_one(a.as_ref(), None, dry_run);
    }
    if applied {
        println!("\n  refreshing version ledger after successful update …");
        cmd_refresh(false)?;
    }
    Ok(())
}

fn run_one(adapter: &dyn Adapter, pkg: Option<&str>, dry_run: bool) -> bool {
    let argv = adapter.upgrade_cmd(pkg);
    if argv.is_empty() {
        return false;
    }
    if dry_run {
        println!("  [dry-run] [{}] {}", adapter.name(), argv.join(" "));
        return false;
    }
    println!("  [{}] {} …", adapter.name(), argv.join(" "));
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let (rc, out, err) = run::run(&refs, 300);
    if rc != 0 {
        let msg = if err.is_empty() { out } else { err };
        println!(
            "    {} {}",
            c("error:", RED),
            msg.lines().next().unwrap_or("")
        );
    } else {
        println!("    {}", c("ok", GREEN));
    }
    rc == 0
}

fn cmd_self(apply: bool) -> Result<()> {
    println!("\n{} — manager self-update\n", c("pum self", BOLD));
    let mut applied = false;
    for a in live_adapters() {
        let argv = a.self_update_cmd();
        if argv.is_empty() {
            continue;
        }
        if apply {
            println!("  [{}] {} …", a.name(), argv.join(" "));
            let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            let (rc, out, err) = run::run(&refs, 300);
            if rc != 0 {
                println!(
                    "    {} {}",
                    c("error:", RED),
                    (if err.is_empty() { out } else { err })
                        .lines()
                        .next()
                        .unwrap_or("")
                );
            } else {
                println!("    {}", c("ok", GREEN));
                applied = true;
            }
        } else {
            println!("  [{}] would run: {}", a.name(), argv.join(" "));
        }
    }
    if !apply {
        println!("\n  Run with --apply to execute.\n");
    } else if applied {
        println!("\n  refreshing version ledger after manager self-update …");
        cmd_refresh(false)?;
    }
    Ok(())
}

const LAUNCH_AGENT_LABEL: &str = "dev.supersynergy.pum.refresh";

fn launch_agent_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| anyhow::anyhow!("HOME is required to install the daily macOS LaunchAgent"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn daily_schedule_plist(program: &str, log: &str, hour: u8, minute: u8) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key><array>
    <string>{program}</string><string>refresh</string><string>--json</string>
  </array>
  <key>StartCalendarInterval</key><dict>
    <key>Hour</key><integer>{hour}</integer>
    <key>Minute</key><integer>{minute}</integer>
  </dict>
  <key>ProcessType</key><string>Background</string>
  <key>ThrottleInterval</key><integer>900</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"#,
        program = xml_escape(program),
        log = xml_escape(log),
    )
}

fn launchd_domain() -> Result<String> {
    let (rc, out, err) = run::run(&["id", "-u"], 10);
    if rc != 0 {
        bail!("could not determine launchd user id: {}", err.trim());
    }
    let uid = out.trim();
    if uid.is_empty() || !uid.bytes().all(|b| b.is_ascii_digit()) {
        bail!("could not determine numeric launchd user id");
    }
    Ok(format!("gui/{uid}"))
}

fn cmd_schedule(install: bool, remove: bool, hour: u8, minute: u8) -> Result<()> {
    if hour > 23 || minute > 59 {
        bail!("--hour must be 0..23 and --minute must be 0..59");
    }
    if !cfg!(target_os = "macos") {
        bail!("pum schedule uses launchd and is currently available on macOS only");
    }
    let plist = launch_agent_path()?;
    if remove {
        let domain = launchd_domain()?;
        let plist_arg = plist.display().to_string();
        let _ = run::run(&["launchctl", "bootout", &domain, &plist_arg], 20);
        if plist.exists() {
            std::fs::remove_file(&plist)?;
        }
        println!("removed daily refresh schedule: {}", plist.display());
        return Ok(());
    }

    if !install {
        println!(
            "daily source check is not installed. Run: pum schedule --install --hour {hour} --minute {minute}"
        );
        return Ok(());
    }

    let program = std::env::current_exe()?;
    let log = data_dir().join("pum-refresh.log");
    let parent = plist
        .parent()
        .ok_or_else(|| anyhow::anyhow!("LaunchAgent path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    std::fs::create_dir_all(data_dir())?;
    std::fs::write(
        &plist,
        daily_schedule_plist(
            &program.display().to_string(),
            &log.display().to_string(),
            hour,
            minute,
        ),
    )?;

    let domain = launchd_domain()?;
    let plist_arg = plist.display().to_string();
    let _ = run::run(&["launchctl", "bootout", &domain, &plist_arg], 20);
    let (rc, _out, err) = run::run(&["launchctl", "bootstrap", &domain, &plist_arg], 20);
    if rc != 0 {
        bail!("launchctl bootstrap failed: {}", err.trim());
    }
    println!(
        "daily source check installed: {:02}:{:02} → {}",
        hour,
        minute,
        plist.display()
    );
    Ok(())
}

fn resolve_dir(path: Option<&str>) -> PathBuf {
    match path {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

fn cmd_project(path: Option<&str>, json: bool) -> Result<()> {
    let dir = resolve_dir(path);
    // A broken/absent toolchain in the project must never abort pum.
    let pkgs =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| project::scan_project(&dir)))
            .unwrap_or_default();
    let outdated: Vec<_> = pkgs
        .into_iter()
        .filter(|p| p.status.as_deref() == Some("outdated"))
        .collect();
    let deps = project::enrich(&outdated);

    if json {
        println!("{}", serde_json::to_string_pretty(&deps)?);
        return Ok(());
    }

    println!(
        "\n{} — {}\n",
        c("pum project", BOLD),
        c(&dir.display().to_string(), DIM)
    );
    if deps.is_empty() {
        println!("  {} no outdated project dependencies\n", c("✓", GREEN));
        return Ok(());
    }

    let w_pkg = deps.iter().map(|p| p.name.len()).max().unwrap_or(7).max(7);
    let w_inst = deps
        .iter()
        .map(|p| p.installed.len())
        .max()
        .unwrap_or(9)
        .max(9);
    for p in &deps {
        let note = if p.deprecated.is_some() {
            format!("  {}", c("[deprecated]", RED))
        } else {
            String::new()
        };
        println!(
            "  {} {:<6} {:<w_pkg$}  {:<w_inst$} → {}{}",
            c("!", YELLOW),
            p.manager,
            p.name,
            p.installed,
            c(&p.latest, YELLOW),
            note,
        );
    }
    let dep_count = deps.iter().filter(|p| p.deprecated.is_some()).count();
    let tail = if dep_count > 0 {
        format!(", {}", c(&format!("{dep_count} deprecated"), RED))
    } else {
        String::new()
    };
    println!(
        "\n  {} update(s) available{}\n",
        c(&deps.len().to_string(), YELLOW),
        tail
    );
    Ok(())
}

fn cmd_audit(path: Option<&str>, json: bool) -> Result<()> {
    let dir = resolve_dir(path);
    let result = audit::audit_project(&dir);

    if json {
        match result {
            Ok(vulns) => println!("{}", serde_json::to_string_pretty(&vulns)?),
            Err(e) => println!("{}", serde_json::json!({ "error": e })),
        }
        return Ok(());
    }

    println!(
        "\n{} — {} {}\n",
        c("pum audit", BOLD),
        c(&dir.display().to_string(), DIM),
        c("(OSV.dev)", DIM)
    );
    match result {
        Ok(vulns) if vulns.is_empty() => {
            println!("  {} no known vulnerabilities\n", c("✓", GREEN));
        }
        Ok(vulns) => {
            for v in &vulns {
                let sev = v.severity.as_deref().unwrap_or("?");
                let fix = if v.fixed.is_empty() {
                    "no fix".to_string()
                } else {
                    format!("fix: {}", v.fixed.join(", "))
                };
                println!(
                    "  {} {}@{}  [{}]  {}  {}",
                    c("⚠", RED),
                    v.package,
                    v.version,
                    c(sev, RED),
                    c(&v.ids.join(", "), YELLOW),
                    c(&fix, DIM),
                );
            }
            let total: usize = vulns.iter().map(|v| v.ids.len()).sum();
            println!(
                "\n  {} advisory(ies) across {} package(s)\n",
                c(&total.to_string(), RED),
                vulns.len()
            );
        }
        Err(e) => println!("  {} {}\n", c("audit failed:", RED), e),
    }
    Ok(())
}

// ── cli ───────────────────────────────────────────────────────────────────--
#[derive(Parser)]
#[command(
    name = "pum",
    version,
    about = "Package Update Manager — packages & tools only, never the OS"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Detect managers, inventory installed packages + versions
    Scan,
    /// Check each manager for available updates
    Check,
    /// Scan then check sources, persist a DuckDB version snapshot
    Refresh {
        #[arg(long)]
        json: bool,
    },
    /// Show freshness, latest-version candidates, and source coverage
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show the inventory table (installed vs latest)
    Report {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        outdated: bool,
        #[arg(long, short)]
        manager: Option<String>,
    },
    /// Upgrade packages (per-tool, --manager, or --all)
    Update {
        packages: Vec<String>,
        #[arg(long, short)]
        manager: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Required to run a report-only manager (none by default)
        #[arg(long)]
        apply: bool,
    },
    /// Check/update the managers themselves
    #[command(name = "self")]
    SelfUpdate {
        #[arg(long)]
        apply: bool,
    },
    /// Which adapters are live on this host
    Doctor,
    /// Install/remove a daily, read-only macOS source check (launchd)
    Schedule {
        #[arg(long, conflicts_with = "remove")]
        install: bool,
        #[arg(long, conflicts_with = "install")]
        remove: bool,
        #[arg(long, default_value_t = 9)]
        hour: u8,
        #[arg(long, default_value_t = 5)]
        minute: u8,
    },
    /// Scan a project's manifest for outdated dependencies (default: cwd)
    Project {
        /// Project directory (defaults to the current directory)
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Security-audit a project's dependencies against OSV.dev (CVE/GHSA)
    Audit {
        /// Project directory (defaults to the current directory)
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Serve safe freshness and update-plan tools over MCP stdio
    Mcp,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Scan => cmd_scan(),
        Cmd::Check => cmd_check(),
        Cmd::Refresh { json } => cmd_refresh(json),
        Cmd::Status { json } => cmd_status(json),
        Cmd::Report {
            json,
            outdated,
            manager,
        } => cmd_report(json, outdated, manager.as_deref()),
        Cmd::Update {
            packages,
            manager,
            all,
            dry_run,
            apply,
        } => cmd_update(&packages, manager.as_deref(), all, dry_run, apply),
        Cmd::SelfUpdate { apply } => cmd_self(apply),
        Cmd::Doctor => cmd_doctor(),
        Cmd::Schedule {
            install,
            remove,
            hour,
            minute,
        } => cmd_schedule(install, remove, hour, minute),
        Cmd::Project { path, json } => cmd_project(path.as_deref(), json),
        Cmd::Audit { path, json } => cmd_audit(path.as_deref(), json),
        Cmd::Mcp => mcp::serve(),
    };
    if let Err(e) = result {
        eprintln!("{} {e}", c("error:", RED));
        std::process::exit(1);
    }
}

// ── tests (pure parse logic, no subprocess) ─────────────────────────────────-
#[cfg(test)]
mod tests {
    use crate::adapters;
    use crate::types::Package;

    fn pkg(manager: &str, name: &str, installed: &str, status: Option<&str>) -> Package {
        Package {
            manager: manager.to_string(),
            name: name.to_string(),
            installed: installed.to_string(),
            latest: None,
            status: status.map(|s| s.to_string()),
            source: String::new(),
            checked_at: String::new(),
        }
    }

    #[test]
    fn pnpm_outdated_ignores_error_line() {
        let out = " ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND  No package.json (or package.yaml, or package.json5) was found in \"/x\".\n";
        assert!(adapters::pnpm::parse_pnpm_outdated(out).is_empty());
    }

    #[test]
    fn pnpm_outdated_parses_real_table() {
        let out = "Package  Current  Latest\nfoo      1.0.0    1.2.0\n";
        let p = adapters::pnpm::parse_pnpm_outdated(out);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "foo");
    }

    #[test]
    fn prune_stale_removes_ghost_row_after_upgrade() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        super::init_schema(&conn).unwrap();

        // v1 installed + a `check` marks it outdated (mirrors the real scan→check flow)
        super::upsert(&conn, &[pkg("brew", "ab-av1", "0.11.2", Some("outdated"))]).unwrap();
        // package gets upgraded; a fresh `scan` now reports v2 installed
        let fresh = [pkg("brew", "ab-av1", "0.11.4", None)];
        super::upsert(&conn, &fresh).unwrap();
        super::prune_stale(&conn, &fresh).unwrap();

        let rows = super::load_all(&conn).unwrap();
        assert_eq!(rows.len(), 1, "the stale 0.11.2 ghost row must be gone");
        assert_eq!(rows[0].installed, "0.11.4");
    }

    #[test]
    fn prune_stale_keeps_multi_version_adapter_rows() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        super::init_schema(&conn).unwrap();

        super::upsert(&conn, &[pkg("mise", "python", "3.12.0", None)]).unwrap();
        let fresh = [pkg("mise", "python", "3.14.0", None)];
        super::upsert(&conn, &fresh).unwrap();
        super::prune_stale(&conn, &fresh).unwrap();

        let rows = super::load_all(&conn).unwrap();
        assert_eq!(
            rows.len(),
            2,
            "mise legitimately keeps multiple installed versions"
        );
    }

    #[test]
    fn prune_stale_removes_tapped_brew_alias_after_upgrade() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        super::init_schema(&conn).unwrap();
        super::upsert(
            &conn,
            &[pkg(
                "brew",
                "ariga/tap/atlas",
                "v1.2.4-old",
                Some("outdated"),
            )],
        )
        .unwrap();
        let fresh = [pkg("brew", "atlas", "v1.2.4-new", None)];
        super::upsert(&conn, &fresh).unwrap();
        super::prune_stale(&conn, &fresh).unwrap();

        let rows = super::load_all(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "atlas");
    }

    #[test]
    fn refresh_history_preserves_latest_candidates() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        super::init_schema(&conn).unwrap();
        let packages = vec![Package::outdated(
            "npm",
            "pum-test",
            "1.0.0",
            "1.1.0",
            "npm-global",
        )];
        super::upsert(&conn, &packages).unwrap();
        let summary = super::record_refresh(
            &conn,
            "2026-07-23T00:00:00Z",
            &super::load_all(&conn).unwrap(),
            1,
            &[],
        )
        .unwrap();
        assert_eq!(summary.update_count, 1);
        let status = super::load_status(&conn).unwrap();
        assert_eq!(status.outdated_count, 1);
        assert_eq!(status.latest_candidates[0].latest.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn daily_schedule_runs_refresh_without_updates() {
        let plist = super::daily_schedule_plist("/tmp/pum", "/tmp/pum.log", 9, 5);
        assert!(plist.contains("<string>refresh</string>"));
        assert!(plist.contains("<string>--json</string>"));
        assert!(!plist.contains("<string>update</string>"));
    }

    #[test]
    fn brew_outdated_parses() {
        let j = r#"{"formulae":[{"name":"foo","installed_versions":["1.0"],"current_version":"1.1"}],"casks":[]}"#;
        let p = adapters::brew::parse_brew_outdated(j);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "foo");
        assert_eq!(p[0].installed, "1.0");
        assert_eq!(p[0].latest.as_deref(), Some("1.1"));
        assert_eq!(p[0].status.as_deref(), Some("outdated"));
    }

    #[test]
    fn brew_outdated_normalizes_tapped_formula_name() {
        let j = r#"{"formulae":[{"name":"ariga/tap/atlas","installed_versions":["1.0"],"current_version":"1.1"}],"casks":[]}"#;
        let p = adapters::brew::parse_brew_outdated(j);
        assert_eq!(p[0].name, "atlas");
    }

    #[test]
    fn brew_outdated_cask_installed_versions_is_an_array() {
        // installed_versions is an array for casks too — was being read as a
        // string and always falling back to "unknown".
        let j = r#"{"formulae":[],"casks":[{"name":"foo-cask","installed_versions":["3.2.3"],"current_version":"3.3.0"}]}"#;
        let p = adapters::brew::parse_brew_outdated(j);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].installed, "3.2.3");
        assert_eq!(p[0].latest.as_deref(), Some("3.3.0"));
    }

    #[test]
    fn brew_installed_casks_keep_an_upgrade_visible_to_pruning() {
        let j = r#"{"casks":[{"token":"demo-cask","installed":"2.0.0","version":"2.0.0"}]}"#;
        let p = adapters::brew::parse_brew_installed_casks(j);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "demo-cask");
        assert_eq!(p[0].installed, "2.0.0");
        assert_eq!(p[0].source, "brew-cask");
    }

    #[test]
    fn brew_outdated_bad_json_is_empty() {
        assert!(adapters::brew::parse_brew_outdated("not json").is_empty());
    }

    #[test]
    fn cargo_installed_and_outdated() {
        let inst = adapters::cargo::parse_cargo_installed("ripgrep v14.1.0:\n    rg\n");
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].name, "ripgrep");
        assert_eq!(inst[0].installed, "14.1.0");
        let out =
            adapters::cargo::parse_cargo_outdated("ripgrep 14.0.0 14.1.0 Yes\nfd 9.0.0 9.0.0 No\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].status.as_deref(), Some("outdated"));
        assert_eq!(out[1].status.as_deref(), Some("current"));
    }

    #[test]
    fn gem_list_and_outdated() {
        let list = adapters::gem::parse_gem_list("rake (13.0.6, 12.3.3)\nbundler (2.4.0)\n");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "rake");
        assert_eq!(list[0].installed, "13.0.6");
        let out = adapters::gem::parse_gem_outdated("rake (13.0.6 < 13.1.0)\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].latest.as_deref(), Some("13.1.0"));
    }

    #[test]
    fn rustup_check_parses() {
        let p = adapters::rustup::parse_rustup_check(
            "stable-aarch64-apple-darwin - Update available : 1.95.0 -> 1.96.0\nrustup - Up to date : 1.27.0\n",
        );
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "stable-aarch64-apple-darwin");
        assert_eq!(p[0].installed, "1.95.0");
        assert_eq!(p[0].latest.as_deref(), Some("1.96.0"));
    }

    #[test]
    fn mise_list_and_outdated() {
        let list = adapters::mise::parse_mise_list("node 22.1.0 ~/.tool-versions\npython 3.14.4\n");
        assert_eq!(list.len(), 2);
        let out = adapters::mise::parse_mise_outdated("node 22.1.0 22.2.0 ~/x\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].latest.as_deref(), Some("22.2.0"));
    }

    // ── project + audit (the global-scan blind spot fix) ───────────────────────
    #[test]
    fn bun_outdated_table_parses() {
        let s = "| Package            | Current | Update  | Latest  |\n\
                 |--------------------|---------|---------|---------|\n\
                 | date-fns           | 3.6.0   | 3.6.0   | 4.4.0   |\n\
                 | @types/react (dev) | 19.2.16 | 19.2.17 | 19.2.17 |\n\
                 | zod                | 3.25.76 | 3.25.76 | 3.25.76 |\n";
        let p = crate::project::parse_bun_outdated(s, "/proj");
        assert_eq!(p.len(), 2, "only the two changed rows are outdated");
        let df = p.iter().find(|x| x.name == "date-fns").unwrap();
        assert_eq!(df.installed, "3.6.0");
        assert_eq!(df.latest.as_deref(), Some("4.4.0"));
        assert_eq!(df.manager, "bun");
        assert!(
            p.iter().any(|x| x.name == "@types/react"),
            "(dev) marker stripped"
        );
    }

    #[test]
    fn bun_outdated_garbage_is_empty() {
        assert!(crate::project::parse_bun_outdated("", "/p").is_empty());
        assert!(crate::project::parse_bun_outdated("no pipes here", "/p").is_empty());
    }

    #[test]
    fn npm_like_json_parses() {
        let s = r#"{"left-pad":{"current":"1.0.0","wanted":"1.3.0","latest":"1.3.0"},"ok":{"current":"2.0.0","wanted":"2.0.0","latest":"2.0.0"}}"#;
        let p = crate::project::parse_npm_like_json(s, "npm", "/proj");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "left-pad");
        assert_eq!(p[0].latest.as_deref(), Some("1.3.0"));
    }

    #[test]
    fn cargo_outdated_json_parses() {
        let s = r#"{"dependencies":[{"name":"serde","project":"1.0.1","latest":"1.0.5"},{"name":"anyhow","project":"1.0.0","latest":"1.0.0"},{"name":"foo","project":"0.1.0","latest":"---"}]}"#;
        let p = crate::project::parse_cargo_outdated_json(s, "/proj");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "serde");
        assert_eq!(p[0].latest.as_deref(), Some("1.0.5"));
    }

    #[test]
    fn bun_ls_parses_scoped() {
        let s = "supercalendar@2.0.1\n├── @dnd-kit/core@6.3.1\n├── zod@3.25.76\n└── node_modules (1234)\n";
        let d = crate::audit::parse_bun_ls(s);
        assert!(d.contains(&("@dnd-kit/core".to_string(), "6.3.1".to_string())));
        assert!(d.contains(&("zod".to_string(), "3.25.76".to_string())));
        assert!(!d.iter().any(|(n, _)| n.contains("node_modules")));
    }

    #[test]
    fn osv_batch_maps_by_index() {
        let deps = vec![
            ("left-pad".to_string(), "1.0.0".to_string()),
            ("safe".to_string(), "2.0.0".to_string()),
        ];
        let resp = r#"{"results":[{"vulns":[{"id":"GHSA-aaaa-bbbb-cccc"}]},{}]}"#;
        let v = crate::audit::parse_osv_batch(resp, &deps);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "left-pad");
        assert_eq!(v[0].ids, vec!["GHSA-aaaa-bbbb-cccc".to_string()]);
    }

    #[test]
    fn osv_body_builds() {
        let deps = vec![("zod".to_string(), "3.25.76".to_string())];
        let body = crate::audit::build_osv_body(&deps, "npm");
        assert!(body.contains("\"ecosystem\":\"npm\""));
        assert!(body.contains("\"name\":\"zod\""));
        assert!(body.contains("\"version\":\"3.25.76\""));
    }

    #[test]
    fn osv_detail_parses_severity_and_fixed() {
        let s = r#"{"id":"GHSA-x","database_specific":{"severity":"HIGH"},"affected":[{"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"1.2.3"}]}]}]}"#;
        let (sev, fixed) = crate::audit::parse_osv_detail(s);
        assert_eq!(sev.as_deref(), Some("HIGH"));
        assert_eq!(fixed, vec!["1.2.3".to_string()]);
    }
}
