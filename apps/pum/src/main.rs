//! pum — Package Update Manager (Rust).
//! Mac-first CLI: scan every package manager + dev tool, inventory versions, check
//! updates, update per-tool or all. Packages & tools only — never the OS.
mod adapters;
mod audit;
mod project;
mod run;
mod types;

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rusqlite::{Connection, params};

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
    data_dir().join("inventory.db")
}

fn json_path() -> PathBuf {
    data_dir().join("inventory.json")
}

// ── db ────────────────────────────────────────────────────────────────────--
fn db_connect() -> Result<Connection> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let conn = Connection::open(db_path())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS tools (
            manager TEXT NOT NULL,
            name TEXT NOT NULL,
            installed TEXT NOT NULL DEFAULT '',
            latest TEXT,
            status TEXT,
            source TEXT,
            checked_at TEXT,
            PRIMARY KEY (manager, name, installed)
         );",
    )?;
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
    write_json(&pkgs)?;

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
            run_one(adapter.as_ref(), Some(pkg), dry_run);
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
            }
        }
        return Ok(());
    }

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
        run_one(a.as_ref(), None, dry_run);
    }
    Ok(())
}

fn run_one(adapter: &dyn Adapter, pkg: Option<&str>, dry_run: bool) {
    let argv = adapter.upgrade_cmd(pkg);
    if argv.is_empty() {
        return;
    }
    if dry_run {
        println!("  [dry-run] [{}] {}", adapter.name(), argv.join(" "));
        return;
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
}

fn cmd_self(apply: bool) -> Result<()> {
    println!("\n{} — manager self-update\n", c("pum self", BOLD));
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
            }
        } else {
            println!("  [{}] would run: {}", a.name(), argv.join(" "));
        }
    }
    if !apply {
        println!("\n  Run with --apply to execute.\n");
    }
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
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Scan => cmd_scan(),
        Cmd::Check => cmd_check(),
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
        Cmd::Project { path, json } => cmd_project(path.as_deref(), json),
        Cmd::Audit { path, json } => cmd_audit(path.as_deref(), json),
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
