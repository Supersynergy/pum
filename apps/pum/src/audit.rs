//! Security audit — ghmax-style CVE/GHSA intel for a project's dependencies.
//!
//! Queries the free OSV.dev batch API (no auth) for known vulnerabilities affecting
//! the exact installed versions. Uses `curl` via the subprocess runner to keep pum a
//! single static binary with zero HTTP runtime deps.
use std::path::Path;

use serde_json::{Value, json};

use crate::run::{run, run_in};

#[derive(Debug, Clone)]
pub struct Vuln {
    pub package: String,
    pub version: String,
    pub ids: Vec<String>,
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
    Ok(parse_osv_batch(&resp, &deps))
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
        // Version must start with a digit; name must be a single token.
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

/// Parse the OSV `querybatch` response. Results are returned in query order, so the
/// index maps back to `deps`.
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
                });
            }
        }
    }
    out
}
