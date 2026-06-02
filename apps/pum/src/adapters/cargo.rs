use crate::adapters::Adapter;
use crate::run::{SUBPROCESS_TIMEOUT, run, run_default, which_exists};
use crate::types::Package;

pub struct CargoAdapter;

impl Adapter for CargoAdapter {
    fn name(&self) -> &str {
        "cargo"
    }
    fn binary(&self) -> &str {
        "cargo"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["cargo", "install", "--list"]);
        parse_cargo_installed(&out)
    }

    fn list_outdated(&self) -> Vec<Package> {
        if !which_exists("cargo-install-update") {
            return vec![];
        }
        let (_, out, _) = run(&["cargo", "install-update", "-l"], SUBPROCESS_TIMEOUT * 2);
        parse_cargo_outdated(&out)
    }

    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String> {
        if which_exists("cargo-install-update") {
            match pkg {
                Some(p) => vec!["cargo".into(), "install-update".into(), p.into()],
                None => vec!["cargo".into(), "install-update".into(), "-a".into()],
            }
        } else {
            match pkg {
                Some(p) => vec!["cargo".into(), "install".into(), "--force".into(), p.into()],
                None => vec!["cargo".into(), "install-update".into(), "-a".into()],
            }
        }
    }
}

/// Pure parse: `cargo install --list` output.
pub fn parse_cargo_installed(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with(' ') {
            continue;
        }
        // "name v0.x.y:"
        if let Some(caps) = parse_cargo_header(line) {
            packages.push(Package::new("cargo", &caps.0, &caps.1, "cargo"));
        }
    }
    packages
}

/// Returns (name, version) from a cargo install --list header line.
pub fn parse_cargo_header(line: &str) -> Option<(String, String)> {
    // pattern: "name v?version:"
    let line = line.trim_end_matches(':');
    let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
    if parts.len() < 2 {
        return None;
    }
    let name = parts[0];
    let ver = parts[1].trim().trim_start_matches('v');
    // strip trailing colon if any
    let ver = ver.trim_end_matches(':');
    if ver.is_empty() {
        return None;
    }
    Some((name.to_string(), ver.to_string()))
}

/// Pure parse: `cargo install-update -l` output.
pub fn parse_cargo_outdated(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        // format: name  current  latest  needsUpdate(Yes/No)
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let needs = parts[3].to_lowercase();
            if needs == "yes" || needs == "no" {
                let status = if needs == "yes" {
                    "outdated"
                } else {
                    "current"
                };
                let mut pkg = Package::new("cargo", parts[0], parts[1], "cargo");
                pkg.latest = Some(parts[2].to_string());
                pkg.status = Some(status.to_string());
                packages.push(pkg);
            }
        }
    }
    packages
}
