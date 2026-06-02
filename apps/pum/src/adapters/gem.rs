use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct GemAdapter;

impl Adapter for GemAdapter {
    fn name(&self) -> &str {
        "gem"
    }
    fn binary(&self) -> &str {
        "gem"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["gem", "list"]);
        parse_gem_list(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["gem", "outdated"]);
        parse_gem_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["gem".into(), "update".into(), p.into()],
            None => vec!["gem".into(), "update".into()],
        }
    }
}

/// Pure parse: `gem list` lines like "name (1.2.3, 1.2.0)".
pub fn parse_gem_list(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let (name, rest) = match line.split_once(" (") {
            Some(x) => x,
            None => continue,
        };
        if name.is_empty() {
            continue;
        }
        let versions = rest.trim_end_matches(')');
        let installed = versions.split(',').next().unwrap_or("unknown").trim();
        packages.push(Package::new("gem", name, installed, "gem"));
    }
    packages
}

/// Pure parse: `gem outdated` lines like "name (1.2.0 < 1.3.0)".
pub fn parse_gem_outdated(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let (name, rest) = match line.split_once(" (") {
            Some(x) => x,
            None => continue,
        };
        let inner = rest.trim_end_matches(')');
        if let Some((installed, latest)) = inner.split_once('<') {
            packages.push(Package::outdated(
                "gem",
                name,
                installed.trim(),
                latest.trim(),
                "gem",
            ));
        }
    }
    packages
}
