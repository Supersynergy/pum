use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;
use serde_json::Value;

pub struct PipxAdapter;

impl Adapter for PipxAdapter {
    fn name(&self) -> &str {
        "pipx"
    }
    fn binary(&self) -> &str {
        "pipx"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["pipx", "list", "--json"]);
        parse_pipx_installed(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        // pipx has no native outdated json
        vec![]
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec!["pipx".into(), "upgrade".into(), p.into()],
            None => vec!["pipx".into(), "upgrade-all".into()],
        }
    }
}

pub fn parse_pipx_installed(json_str: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    if let Some(venvs) = data.get("venvs").and_then(|v| v.as_object()) {
        for (name, info) in venvs {
            let version = info
                .get("metadata")
                .and_then(|m| m.get("main_package"))
                .and_then(|p| p.get("package_version"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            packages.push(Package::new("pipx", name, version, "pipx"));
        }
    }
    packages
}
