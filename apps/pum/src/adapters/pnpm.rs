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
        let mut packages = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && !line.starts_with("Package") {
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

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["pnpm".into(), "up".into(), "-g".into(), p.into()],
            None => vec!["pnpm".into(), "up".into(), "-g".into()],
        }
    }
}
