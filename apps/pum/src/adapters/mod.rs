pub mod brew;
pub mod bun;
pub mod cargo;
pub mod gem;
pub mod gh;
pub mod go;
pub mod mise;
pub mod npm;
pub mod pipx;
pub mod pnpm;
pub mod rustup;
pub mod uv;

pub use brew::BrewAdapter;
pub use bun::BunAdapter;
pub use cargo::CargoAdapter;
pub use gem::GemAdapter;
pub use gh::GhAdapter;
pub use go::GoAdapter;
pub use mise::MiseAdapter;
pub use npm::NpmAdapter;
pub use pipx::PipxAdapter;
pub use pnpm::PnpmAdapter;
pub use rustup::RustupAdapter;
pub use uv::UvAdapter;

use crate::types::Package;

/// The Adapter trait mirrors the Python Adapter base class exactly.
pub trait Adapter: Send + Sync {
    fn name(&self) -> &str;
    fn binary(&self) -> &str;
    fn detect(&self) -> bool {
        crate::run::which_exists(self.binary())
    }
    fn list_installed(&self) -> Vec<Package>;
    fn list_outdated(&self) -> Vec<Package>;
    fn upgrade_cmd(&self, pkg: Option<&str>) -> Vec<String>;
    fn self_update_cmd(&self) -> Vec<String> {
        vec![]
    }
    fn report_only(&self) -> bool {
        false
    }
    /// True only for adapters where multiple installed versions of the same
    /// package name legitimately coexist (mise per-project tool versions,
    /// rustup toolchains). For every other adapter, an install replaces the
    /// prior version — a stale (manager,name,old_installed) row must not
    /// survive a re-scan, or `report --outdated` shows ghosts.
    fn multi_version(&self) -> bool {
        false
    }
}

/// Return all 12 adapters (no softwareupdate / no OS).
pub fn all_adapters() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(BrewAdapter),
        Box::new(NpmAdapter),
        Box::new(PnpmAdapter),
        Box::new(BunAdapter),
        Box::new(UvAdapter),
        Box::new(PipxAdapter),
        Box::new(CargoAdapter),
        Box::new(RustupAdapter),
        Box::new(GemAdapter),
        Box::new(GoAdapter),
        Box::new(MiseAdapter),
        Box::new(GhAdapter),
    ]
}

pub fn live_adapters() -> Vec<Box<dyn Adapter>> {
    all_adapters().into_iter().filter(|a| a.detect()).collect()
}

pub fn get_adapter(name: &str) -> Option<Box<dyn Adapter>> {
    all_adapters().into_iter().find(|a| a.name() == name)
}
