use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct BunAdapter;

impl Adapter for BunAdapter {
    fn name(&self) -> &str {
        "bun"
    }
    fn binary(&self) -> &str {
        "bun"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["bun", "pm", "ls", "-g"]);
        let mut packages = Vec::new();
        for line in out.lines() {
            // strip tree chars: └─ ├─ and leading spaces
            let line = line.trim().trim_start_matches(['└', '─', '├', ' ']);
            // format: name@version — skip bun itself
            if line.contains('@')
                && !line.starts_with("bun")
                && let Some(at) = line.rfind('@')
                && at > 0
            {
                let name = &line[..at];
                let ver = &line[at + 1..];
                packages.push(Package::new("bun", name, ver, "bun-global"));
            }
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        // bun has no native outdated
        vec![]
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["bun".into(), "add".into(), "-g".into(), p.into()],
            None => vec!["bun".into(), "upgrade".into()],
        }
    }

    fn self_update_cmd(&self) -> Vec<String> {
        vec!["bun".into(), "upgrade".into()]
    }
}
