//! Project-local dependency scanning.
//!
//! The global adapters only ever see `-g` installs, so outdated deps declared in a
//! repo's manifest (package.json / Cargo.toml) were invisible. This module reads the
//! manifest in a given directory and runs the ecosystem's native `outdated` command.
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::run::{run, run_in};
use crate::types::Package;

/// An outdated project dependency, optionally flagged as deprecated upstream.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDep {
    pub manager: String,
    pub name: String,
    pub installed: String,
    pub latest: String,
    pub deprecated: Option<String>,
}

/// Enrich outdated packages with upstream deprecation notes.
/// Runs in parallel; only node-ecosystem packages are checked (npm registry).
pub fn enrich(outdated: &[Package]) -> Vec<ProjectDep> {
    use rayon::prelude::*;
    outdated
        .par_iter()
        .map(|p| {
            let deprecated = if matches!(p.manager.as_str(), "bun" | "npm" | "pnpm") {
                npm_deprecation(&p.name, &p.installed)
            } else {
                None
            };
            ProjectDep {
                manager: p.manager.clone(),
                name: p.name.clone(),
                installed: p.installed.clone(),
                latest: p.latest.clone().unwrap_or_default(),
                deprecated,
            }
        })
        .collect()
}

/// `npm view <name>@<version> deprecated` → the deprecation message, if any.
fn npm_deprecation(name: &str, version: &str) -> Option<String> {
    let spec = format!("{name}@{version}");
    let (rc, out, _) = run(&["npm", "view", &spec, "deprecated"], 30);
    if rc != 0 {
        return None;
    }
    let t = out.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Scan a project directory for outdated dependencies across detected ecosystems.
pub fn scan_project(dir: &Path) -> Vec<Package> {
    let mut out = Vec::new();
    let has = |f: &str| dir.join(f).exists();
    let src = dir.display().to_string();

    // Node ecosystem — first matching lockfile wins.
    if has("bun.lock") || has("bun.lockb") {
        let (_, o, _) = run_in(dir, &["bun", "outdated"], 120);
        out.extend(parse_bun_outdated(&o, &src));
    } else if has("pnpm-lock.yaml") {
        let (_, o, _) = run_in(dir, &["pnpm", "outdated", "--format", "json"], 120);
        out.extend(parse_npm_like_json(&o, "pnpm", &src));
    } else if has("package-lock.json") || has("package.json") {
        let (_, o, _) = run_in(dir, &["npm", "outdated", "--json"], 120);
        out.extend(parse_npm_like_json(&o, "npm", &src));
    }

    // Rust ecosystem (needs cargo-outdated installed; absent → empty, never panics).
    if has("Cargo.toml") {
        let (_, o, _) = run_in(dir, &["cargo", "outdated", "--format", "json"], 180);
        out.extend(parse_cargo_outdated_json(&o, &src));
    }

    out
}

/// Parse the `bun outdated` table (Package | Current | Update | Latest).
pub fn parse_bun_outdated(out: &str, src: &str) -> Vec<Package> {
    let mut v = Vec::new();
    for line in out.lines() {
        if !line.contains('|') && !line.contains('│') {
            continue;
        }
        let norm = line.replace('│', "|");
        let cells: Vec<String> = norm
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        if cells.len() < 4 {
            continue;
        }
        let name_raw = &cells[0];
        if name_raw.eq_ignore_ascii_case("Package") {
            continue; // header
        }
        if name_raw.chars().all(|ch| ch == '-' || ch == '─') {
            continue; // separator
        }
        // Strip trailing markers like " (dev)" / " (peer)".
        let name = name_raw.split_whitespace().next().unwrap_or(name_raw);
        let current = &cells[1];
        let latest = &cells[3];
        if !current.is_empty() && !latest.is_empty() && current != latest {
            v.push(Package::outdated("bun", name, current, latest, src));
        }
    }
    v
}

/// Parse `npm outdated --json` / `pnpm outdated --format json` (object keyed by name).
pub fn parse_npm_like_json(json_str: &str, mgr: &str, src: &str) -> Vec<Package> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut v = Vec::new();
    if let Some(obj) = data.as_object() {
        for (name, info) in obj {
            let current = info.get("current").and_then(|x| x.as_str()).unwrap_or("");
            let latest = info.get("latest").and_then(|x| x.as_str()).unwrap_or("");
            if !latest.is_empty() && current != latest {
                let installed = if current.is_empty() { "—" } else { current };
                v.push(Package::outdated(mgr, name, installed, latest, src));
            }
        }
    }
    v
}

/// Parse `cargo outdated --format json` ({"dependencies":[{name,project,latest}]}).
pub fn parse_cargo_outdated_json(json_str: &str, src: &str) -> Vec<Package> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut v = Vec::new();
    if let Some(deps) = data.get("dependencies").and_then(|d| d.as_array()) {
        for d in deps {
            let name = d.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let project = d.get("project").and_then(|x| x.as_str()).unwrap_or("");
            let latest = d.get("latest").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() || latest.is_empty() || latest == "---" {
                continue;
            }
            if project != latest {
                v.push(Package::outdated("cargo", name, project, latest, src));
            }
        }
    }
    v
}
