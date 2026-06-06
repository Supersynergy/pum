//! Security audit — ghmax-style CVE/GHSA intel for a project's dependencies.
//!
//! Queries the free OSV.dev API (no auth) for known vulnerabilities affecting the exact
//! installed versions, then enriches each advisory with severity + fixed version. Uses
//! `curl` via the subprocess runner to keep pum a single static binary (zero HTTP deps).
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::run::{run, run_in};

#[derive(Debug, Clone, Serialize)]
pub struct Vuln {
    pub package: String,
    pub version: String,
    pub ids: Vec<String>,
    pub severity: Option<String>,
    pub fixed: Vec<String>,
}

/// Audit a project's node dependencies against the OSV database.
/// Returns `Err` only on network/tool failure; an empty Vec means "no known vulns".
pub fn audit_project(dir: &Path) -> Result<Vec<Vuln>, String> {
    let (_, ls, _) = run_in(dir, &["bun", "pm", "ls"], 60);
    let deps = parse_bun_ls(&ls);
    if deps.is_empty() {
        return Ok(vec![]);
    }
    let body = build_osv_body(&deps, "npm");
    let (rc, resp, err) = run(
        &[
            "curl",
            "-sS",
            "--max-time",
            "30",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "https://api.osv.dev/v1/querybatch",
        ],
        40,
    );
    if rc != 0 {
        let msg = if err.is_empty() { resp } else { err };
        return Err(msg.lines().next().unwrap_or("curl failed").to_string());
    }

    let mut vulns = parse_osv_batch(&resp, &deps);
    // Enrich: the batch endpoint returns only ids; fetch each advisory for severity + fix.
    for v in vulns.iter_mut() {
        let mut severity: Option<String> = None;
        let mut fixed: Vec<String> = Vec::new();
        for id in &v.ids {
            let url = format!("https://api.osv.dev/v1/vulns/{id}");
            let (drc, dresp, _) = run(&["curl", "-sS", "--max-time", "20", &url], 25);
            if drc != 0 {
                continue;
            }
            let (sev, fx) = parse_osv_detail(&dresp);
            if severity.is_none() {
                severity = sev;
            }
            for f in fx {
                if !fixed.contains(&f) {
                    fixed.push(f);
                }
            }
        }
        v.severity = severity;
        v.fixed = fixed;
    }
    Ok(vulns)
}

/// Parse `bun pm ls` into (name, version) pairs (handles scoped packages).
pub fn parse_bun_ls(out: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for line in out.lines() {
        let line = line.trim().trim_start_matches(['└', '─', '├', '│', ' ']);
        let Some(at) = line.rfind('@') else { continue };
        if at == 0 {
            continue; // bare "@scope" with no version
        }
        let name = &line[..at];
        let ver = &line[at + 1..];
        if name.contains(char::is_whitespace) {
            continue;
        }
        if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            v.push((name.to_string(), ver.to_string()));
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Build an OSV `querybatch` request body for the given deps.
pub fn build_osv_body(deps: &[(String, String)], ecosystem: &str) -> String {
    let queries: Vec<Value> = deps
        .iter()
        .map(|(name, ver)| json!({"package": {"name": name, "ecosystem": ecosystem}, "version": ver}))
        .collect();
    json!({ "queries": queries }).to_string()
}

/// Parse the OSV `querybatch` response. Results are in query order → index maps to deps.
pub fn parse_osv_batch(resp: &str, deps: &[(String, String)]) -> Vec<Vuln> {
    let data: Value = match serde_json::from_str(resp) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut out = Vec::new();
    if let Some(results) = data.get("results").and_then(|r| r.as_array()) {
        for (i, res) in results.iter().enumerate() {
            let Some(vulns) = res.get("vulns").and_then(|v| v.as_array()) else {
                continue;
            };
            if vulns.is_empty() {
                continue;
            }
            let ids: Vec<String> = vulns
                .iter()
                .filter_map(|v| v.get("id").and_then(|x| x.as_str()).map(String::from))
                .collect();
            if let Some((name, ver)) = deps.get(i) {
                out.push(Vuln {
                    package: name.clone(),
                    version: ver.clone(),
                    ids,
                    severity: None,
                    fixed: Vec::new(),
                });
            }
        }
    }
    out
}

/// Parse an OSV advisory detail → (severity label, fixed versions).
pub fn parse_osv_detail(json_str: &str) -> (Option<String>, Vec<String>) {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (None, vec![]),
    };

    // Prefer the human label (e.g. "HIGH"); fall back to the CVSS vector/score.
    let mut severity = data
        .get("database_specific")
        .and_then(|d| d.get("severity"))
        .and_then(|s| s.as_str())
        .map(String::from);
    if severity.is_none() {
        severity = data
            .get("severity")
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.get("score"))
            .and_then(|s| s.as_str())
            .map(String::from);
    }

    let mut fixed = Vec::new();
    if let Some(affected) = data.get("affected").and_then(|a| a.as_array()) {
        for aff in affected {
            let Some(ranges) = aff.get("ranges").and_then(|r| r.as_array()) else {
                continue;
            };
            for rng in ranges {
                let Some(events) = rng.get("events").and_then(|e| e.as_array()) else {
                    continue;
                };
                for ev in events {
                    if let Some(fx) = ev.get("fixed").and_then(|f| f.as_str()) {
                        let fx = fx.to_string();
                        if !fixed.contains(&fx) {
                            fixed.push(fx);
                        }
                    }
                }
            }
        }
    }
    (severity, fixed)
}
