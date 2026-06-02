use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct UvAdapter;

impl Adapter for UvAdapter {
    fn name(&self) -> &str {
        "uv"
    }
    fn binary(&self) -> &str {
        "uv"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["uv", "tool", "list"]);
        let mut packages = Vec::new();
        for line in out.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('-') {
                continue;
            }
            // format: "name v0.x.y" or "name 0.x.y"
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                let name = parts[0];
                let ver = parts[1].trim().trim_start_matches('v');
                packages.push(Package::new("uv", name, ver, "uv-tool"));
            }
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        // uv has no direct outdated list; upgrade --all handles it
        vec![]
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["uv".into(), "tool".into(), "upgrade".into(), p.into()],
            None => vec!["uv".into(), "tool".into(), "upgrade".into(), "--all".into()],
        }
    }

    fn self_update_cmd(&self) -> Vec<String> {
        vec!["uv".into(), "self".into(), "update".into()]
    }
}
