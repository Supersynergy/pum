use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct GhAdapter;

impl Adapter for GhAdapter {
    fn name(&self) -> &str {
        "gh"
    }
    fn binary(&self) -> &str {
        "gh"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["gh", "extension", "list"]);
        parse_gh_extensions(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        // gh has no outdated query; `gh extension upgrade --all` handles it.
        vec![]
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["gh".into(), "extension".into(), "upgrade".into(), p.into()],
            None => vec![
                "gh".into(),
                "extension".into(),
                "upgrade".into(),
                "--all".into(),
            ],
        }
    }
}

/// Pure parse: `gh extension list` tab-separated rows.
pub fn parse_gh_extensions(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        let name = parts.first().map(|s| s.trim()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let installed = if parts.len() > 2 {
            parts[2].trim()
        } else if parts.len() == 2 {
            parts[1].trim()
        } else {
            "unknown"
        };
        packages.push(Package::new("gh", name, installed, "gh-ext"));
    }
    packages
}
