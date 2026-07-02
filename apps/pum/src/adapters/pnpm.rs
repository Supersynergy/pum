use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct PnpmAdapter;

impl Adapter for PnpmAdapter {
    fn name(&self) -> &str {
        "pnpm"
    }
    fn binary(&self) -> &str {
        "pnpm"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["pnpm", "ls", "-g", "--depth=0"]);
        let mut packages = Vec::new();
        for line in out.lines() {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with("Legend")
                || line.starts_with('/')
                || line.starts_with("dependencies")
            {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && !parts[0].starts_with('-') {
                packages.push(Package::new("pnpm", parts[0], parts[1], "pnpm-global"));
            }
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["pnpm", "outdated", "-g"]);
        parse_pnpm_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["pnpm".into(), "up".into(), "-g".into(), p.into()],
            None => vec!["pnpm".into(), "up".into(), "-g".into()],
        }
    }
}

/// `pnpm outdated -g` prints an `ERR_PNPM_*` error line (not a table) when no
/// global manifest exists — that error text still has 3+ whitespace-separated
/// tokens and was being misparsed as a package named "ERR_PNPM_...".
pub fn parse_pnpm_outdated(out: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && !line.starts_with("Package") && !parts[0].starts_with("ERR_") {
            packages.push(Package::outdated(
                "pnpm",
                parts[0],
                parts[1],
                parts[2],
                "pnpm-global",
            ));
        }
    }
    packages
}
