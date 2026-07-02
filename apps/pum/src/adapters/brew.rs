use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;
use serde_json::Value;

pub struct BrewAdapter;

impl Adapter for BrewAdapter {
    fn name(&self) -> &str {
        "brew"
    }
    fn binary(&self) -> &str {
        "brew"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["brew", "list", "--versions"]);
        let mut packages = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                packages.push(Package::new(
                    "brew",
                    parts[0],
                    parts[parts.len() - 1],
                    "brew",
                ));
            } else if parts.len() == 1 {
                packages.push(Package::new("brew", parts[0], "unknown", "brew"));
            }
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (rc, out, _) = run_default(&["brew", "outdated", "--json=v2"]);
        if rc != 0 {
            return vec![];
        }
        parse_brew_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["brew".into(), "upgrade".into(), p.into()],
            None => vec!["brew".into(), "upgrade".into()],
        }
    }

    fn self_update_cmd(&self) -> Vec<String> {
        vec!["brew".into(), "update".into()]
    }
}

/// Pure parse function (also used in tests).
pub fn parse_brew_outdated(json_str: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    if let Some(formulae) = data.get("formulae").and_then(|v| v.as_array()) {
        for item in formulae {
            let name = match item.get("name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let installed_ver = item
                .get("installed_versions")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.last())
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let latest = item
                .get("current_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            packages.push(Package::outdated(
                "brew",
                name,
                installed_ver,
                latest,
                "brew",
            ));
        }
    }

    if let Some(casks) = data.get("casks").and_then(|v| v.as_array()) {
        for item in casks {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("token").and_then(|v| v.as_str()))
                .unwrap_or("unknown");
            let installed = item
                .get("installed_versions")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.last())
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let latest = item
                .get("current_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            packages.push(Package::outdated(
                "brew",
                name,
                installed,
                latest,
                "brew-cask",
            ));
        }
    }

    packages
}
