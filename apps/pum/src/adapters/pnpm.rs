use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;
use serde_json::Value;

pub struct PnpmAdapter;

impl Adapter for PnpmAdapter {
    fn name(&self) -> &str {
        "pnpm"
    }
    fn binary(&self) -> &str {
        "pnpm"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["pnpm", "ls", "-g", "--depth=0", "--json"]);
        parse_pnpm_installed_json(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["pnpm", "outdated", "-g", "--format", "json"]);
        parse_pnpm_outdated_json(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["pnpm".into(), "up".into(), "-g".into(), p.into()],
            None => vec!["pnpm".into(), "up".into(), "-g".into()],
        }
    }

    fn complete_inventory(&self) -> bool {
        true
    }

    fn source_health_error(&self) -> Option<String> {
        let (_, out, err) = run_default(&["pnpm", "outdated", "-g", "--format", "json"]);
        if pnpm_global_manifest_missing(&out) || pnpm_global_manifest_missing(&err) {
            Some("pnpm: global manifest is missing; freshness is unavailable".to_string())
        } else {
            None
        }
    }
}

/// Parse `pnpm ls -g --depth=0 --json`.
///
/// pnpm 10's human table contains tree-drawing characters and section labels,
/// which were incorrectly persisted as package names. The JSON form is stable
/// and has one root per global directory.
pub fn parse_pnpm_installed_json(json_str: &str) -> Vec<Package> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut packages = Vec::new();
    for root in data.as_array().into_iter().flatten() {
        if let Some(deps) = root.get("dependencies").and_then(Value::as_object) {
            for (name, info) in deps {
                if let Some(version) = info.get("version").and_then(Value::as_str) {
                    packages.push(Package::new("pnpm", name, version, "pnpm-global"));
                }
            }
        }
    }
    packages
}

/// Parse `pnpm outdated -g --format json` (object keyed by package name).
/// Invalid or error output produces no candidates; it is never interpreted as
/// package data.
pub fn parse_pnpm_outdated_json(json_str: &str) -> Vec<Package> {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut packages = Vec::new();
    if let Some(obj) = data.as_object() {
        for (name, info) in obj {
            let installed = info
                .get("current")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let latest = info
                .get("latest")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if latest != "unknown" && installed != latest {
                packages.push(Package::outdated(
                    "pnpm",
                    name,
                    installed,
                    latest,
                    "pnpm-global",
                ));
            }
        }
    }
    packages
}

/// pnpm emits this sentinel when its configured global directory was removed.
/// Treat it as source failure, never as a clean "no updates" result.
pub fn pnpm_global_manifest_missing(text: &str) -> bool {
    text.contains("ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND")
}
