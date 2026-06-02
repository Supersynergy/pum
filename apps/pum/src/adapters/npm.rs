use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;
use serde_json::Value;

pub struct NpmAdapter;

impl Adapter for NpmAdapter {
    fn name(&self) -> &str {
        "npm"
    }
    fn binary(&self) -> &str {
        "npm"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["npm", "ls", "-g", "--depth=0", "--json"]);
        parse_npm_installed(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["npm", "outdated", "-g", "--json"]);
        parse_npm_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            Some(p) => vec![
                "npm".into(),
                "i".into(),
                "-g".into(),
                format!("{}@latest", p),
            ],
            None => vec!["npm".into(), "update".into(), "-g".into()],
        }
    }
}

pub fn parse_npm_installed(json_str: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    if let Some(deps) = data.get("dependencies").and_then(|v| v.as_object()) {
        for (name, info) in deps {
            let version = info
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            packages.push(Package::new("npm", name, version, "npm-global"));
        }
    }
    packages
}

pub fn parse_npm_outdated(json_str: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    if let Some(obj) = data.as_object() {
        for (name, info) in obj {
            let installed = info
                .get("current")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let latest = info
                .get("latest")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            packages.push(Package::outdated(
                "npm",
                name,
                installed,
                latest,
                "npm-global",
            ));
        }
    }
    packages
}
