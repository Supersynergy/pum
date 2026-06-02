use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;
use std::path::PathBuf;

pub struct GoAdapter;

impl Adapter for GoAdapter {
    fn name(&self) -> &str {
        "go"
    }
    fn binary(&self) -> &str {
        "go"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["go", "env", "GOPATH"]);
        let gopath = out.trim();
        let bin_dir = if gopath.is_empty() {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("go/bin")
        } else {
            PathBuf::from(gopath).join("bin")
        };
        let mut packages = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&bin_dir) {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            for name in names {
                // go binaries carry no queryable version
                packages.push(Package::new("go", &name, "unknown", "go-bin"));
            }
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        // go binaries have no standard version check
        vec![]
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        match pkg {
            // needs the full import path; caller supplies it
            Some(p) => vec!["go".into(), "install".into(), format!("{p}@latest")],
            None => vec![], // no bulk upgrade without tracked import paths
        }
    }
}
