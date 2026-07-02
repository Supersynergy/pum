use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct MiseAdapter;

impl Adapter for MiseAdapter {
    fn name(&self) -> &str {
        "mise"
    }
    fn binary(&self) -> &str {
        "mise"
    }
    fn multi_version(&self) -> bool {
        true
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["mise", "ls", "--current"]);
        parse_mise_list(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["mise", "outdated"]);
        parse_mise_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["mise".into(), "upgrade".into(), p.into()],
            None => vec!["mise".into(), "upgrade".into()],
        }
    }
}

/// Pure parse: `mise ls --current` lines "name version [...]".
pub fn parse_mise_list(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            packages.push(Package::new("mise", parts[0], parts[1], "mise"));
        }
    }
    packages
}

/// Pure parse: `mise outdated` lines "name installed latest [...]" (skip header).
pub fn parse_mise_outdated(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        if line.starts_with("Plugin") || line.starts_with("Tool") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            packages.push(Package::outdated(
                "mise", parts[0], parts[1], parts[2], "mise",
            ));
        }
    }
    packages
}
