use crate::adapters::Adapter;
use crate::run::run_default;
use crate::types::Package;

pub struct RustupAdapter;

impl Adapter for RustupAdapter {
    fn name(&self) -> &str {
        "rustup"
    }
    fn binary(&self) -> &str {
        "rustup"
    }

    fn list_installed(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["rustup", "toolchain", "list"]);
        let mut packages = Vec::new();
        for line in out.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // strip "(default)" / "(active)" markers — take first token
            let name = line.split_whitespace().next().unwrap_or(line);
            packages.push(Package::new("rustup", name, name, "rustup"));
        }
        packages
    }

    fn list_outdated(&self) -> Vec<Package> {
        let (_, out, _) = run_default(&["rustup", "check"]);
        parse_rustup_check(&out)
    }

    fn upgrade_cmd(&self, _pkg: Option<&str>) -> Vec<String> {
        vec!["rustup".into(), "update".into()]
    }

    fn self_update_cmd(&self) -> Vec<String> {
        vec!["rustup".into(), "self".into(), "update".into()]
    }
}

/// Pure parse: `rustup check` lines like
/// "stable-aarch64-apple-darwin - Update available : 1.95.0 -> 1.96.0"
pub fn parse_rustup_check(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in text.lines() {
        if !line.contains("Update available") {
            continue;
        }
        let name = match line.split_whitespace().next() {
            Some(n) => n,
            None => continue,
        };
        // installed -> latest  (tokens around "->")
        let (installed, latest) = match line.split_once("->") {
            Some((before, after)) => {
                let installed = before
                    .rsplit(':')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .last()
                    .unwrap_or("unknown");
                let latest = after.split_whitespace().next().unwrap_or("unknown");
                (installed, latest)
            }
            None => ("unknown", "unknown"),
        };
        packages.push(Package::outdated(
            "rustup", name, installed, latest, "rustup",
        ));
    }
    packages
}
