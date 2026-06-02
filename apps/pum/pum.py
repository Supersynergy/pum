#!/usr/bin/env python3
"""pum — Package Update Manager: adapter-based multi-manager update tool."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent
DATA_DIR = PROJECT_ROOT / "data"
DB_PATH = DATA_DIR / "inventory.db"
JSON_PATH = DATA_DIR / "inventory.json"

DATA_DIR.mkdir(parents=True, exist_ok=True)

# ---------------------------------------------------------------------------
# ANSI helpers
# ---------------------------------------------------------------------------

RESET = "\033[0m"
RED = "\033[31m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
BOLD = "\033[1m"
DIM = "\033[2m"
CYAN = "\033[36m"


def _color(text: str, code: str) -> str:
    if sys.stdout.isatty():
        return f"{code}{text}{RESET}"
    return text


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------


@dataclass
class Package:
    manager: str
    name: str
    installed: str = "unknown"
    latest: str = "unknown"
    status: str = "unknown"  # current | outdated | unknown
    source: str = ""
    checked_at: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())


# ---------------------------------------------------------------------------
# subprocess helpers
# ---------------------------------------------------------------------------

SUBPROCESS_TIMEOUT = 60  # seconds per command


def _run(
    argv: list[str], timeout: int = SUBPROCESS_TIMEOUT, env: dict | None = None
) -> tuple[int, str, str]:
    """Run a command; return (returncode, stdout, stderr). Never raises on non-zero."""
    try:
        result = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env or os.environ.copy(),
        )
        return result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired:
        return -1, "", f"timeout after {timeout}s"
    except FileNotFoundError:
        return -1, "", f"binary not found: {argv[0]}"
    except Exception as exc:
        return -1, "", str(exc)


def _which(name: str) -> bool:
    return shutil.which(name) is not None


# ---------------------------------------------------------------------------
# Adapter base
# ---------------------------------------------------------------------------


class Adapter:
    name: str = ""
    binary: str = ""
    # report_only adapters are NEVER upgraded by `pum update --all`; they require an
    # explicit `pum update --manager <name> --apply` (e.g. macOS softwareupdate, which
    # can install OS updates / trigger reboots).
    report_only: bool = False

    def detect(self) -> bool:
        return _which(self.binary)

    def list_installed(self) -> list[Package]:
        return []

    def list_outdated(self) -> list[Package]:
        return []

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        """Return argv to upgrade pkg, or all if pkg is None."""
        return []

    def self_update_cmd(self) -> list[str]:
        return []


# ---------------------------------------------------------------------------
# Adapters
# ---------------------------------------------------------------------------


class BrewAdapter(Adapter):
    name = "brew"
    binary = "brew"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["brew", "list", "--versions"])
        packages = []
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 2:
                packages.append(
                    Package(
                        manager="brew",
                        name=parts[0],
                        installed=parts[-1],
                        source="brew",
                    )
                )
            elif len(parts) == 1:
                packages.append(Package(manager="brew", name=parts[0], source="brew"))
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, err = _run(["brew", "outdated", "--json=v2"])
        if rc != 0:
            return []
        packages = []
        try:
            data = json.loads(out)
            for item in data.get("formulae", []):
                installed = item.get("installed_versions", ["unknown"])
                installed_ver = installed[-1] if installed else "unknown"
                packages.append(
                    Package(
                        manager="brew",
                        name=item["name"],
                        installed=installed_ver,
                        latest=item.get("current_version", "unknown"),
                        status="outdated",
                        source="brew",
                    )
                )
            for item in data.get("casks", []):
                packages.append(
                    Package(
                        manager="brew",
                        name=item.get("name", item.get("token", "unknown")),
                        installed=item.get("installed_versions", "unknown"),
                        latest=item.get("current_version", "unknown"),
                        status="outdated",
                        source="brew-cask",
                    )
                )
        except (json.JSONDecodeError, KeyError):
            pass
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["brew", "upgrade", pkg]
        return ["brew", "upgrade"]

    def self_update_cmd(self) -> list[str]:
        return ["brew", "update"]


class NpmAdapter(Adapter):
    name = "npm"
    binary = "npm"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["npm", "ls", "-g", "--depth=0", "--json"])
        packages = []
        try:
            data = json.loads(out)
            deps = data.get("dependencies", {})
            for name, info in deps.items():
                packages.append(
                    Package(
                        manager="npm",
                        name=name,
                        installed=info.get("version", "unknown"),
                        source="npm-global",
                    )
                )
        except (json.JSONDecodeError, KeyError):
            pass
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["npm", "outdated", "-g", "--json"])
        packages = []
        try:
            data = json.loads(out)
            for name, info in data.items():
                packages.append(
                    Package(
                        manager="npm",
                        name=name,
                        installed=info.get("current", "unknown"),
                        latest=info.get("latest", "unknown"),
                        status="outdated",
                        source="npm-global",
                    )
                )
        except (json.JSONDecodeError, KeyError):
            pass
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["npm", "i", "-g", f"{pkg}@latest"]
        return ["npm", "update", "-g"]


class PnpmAdapter(Adapter):
    name = "pnpm"
    binary = "pnpm"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["pnpm", "ls", "-g", "--depth=0"])
        packages = []
        # pnpm ls -g output: lines like "name version path"
        for line in out.splitlines():
            line = line.strip()
            if (
                not line
                or line.startswith("Legend")
                or line.startswith("/")
                or line.startswith("dependencies")
            ):
                continue
            parts = line.split()
            if len(parts) >= 2 and not parts[0].startswith("-"):
                packages.append(
                    Package(
                        manager="pnpm",
                        name=parts[0],
                        installed=parts[1],
                        source="pnpm-global",
                    )
                )
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["pnpm", "outdated", "-g"])
        packages = []
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 3 and not line.startswith("Package"):
                packages.append(
                    Package(
                        manager="pnpm",
                        name=parts[0],
                        installed=parts[1],
                        latest=parts[2],
                        status="outdated",
                        source="pnpm-global",
                    )
                )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["pnpm", "up", "-g", pkg]
        return ["pnpm", "up", "-g"]


class BunAdapter(Adapter):
    name = "bun"
    binary = "bun"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["bun", "pm", "ls", "-g"])
        packages = []
        for line in out.splitlines():
            line = line.strip().lstrip("└─├─ ")
            if "@" in line and not line.startswith("bun"):
                # format: name@version
                at = line.rfind("@")
                if at > 0:
                    name = line[:at]
                    ver = line[at + 1 :]
                    packages.append(
                        Package(manager="bun", name=name, installed=ver, source="bun-global")
                    )
        return packages

    def list_outdated(self) -> list[Package]:
        # bun has no native outdated; return empty
        return []

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["bun", "add", "-g", pkg]
        return ["bun", "upgrade"]

    def self_update_cmd(self) -> list[str]:
        return ["bun", "upgrade"]


class UvAdapter(Adapter):
    name = "uv"
    binary = "uv"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["uv", "tool", "list"])
        packages = []
        for line in out.splitlines():
            line = line.strip()
            if not line or line.startswith("-"):
                continue
            # format: "name v0.x.y"
            m = re.match(r"^(\S+)\s+v?(\S+)", line)
            if m:
                packages.append(
                    Package(
                        manager="uv",
                        name=m.group(1),
                        installed=m.group(2),
                        source="uv-tool",
                    )
                )
        return packages

    def list_outdated(self) -> list[Package]:
        # uv has no direct outdated list; upgrade --all handles it
        return []

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["uv", "tool", "upgrade", pkg]
        return ["uv", "tool", "upgrade", "--all"]

    def self_update_cmd(self) -> list[str]:
        return ["uv", "self", "update"]


class PipxAdapter(Adapter):
    name = "pipx"
    binary = "pipx"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["pipx", "list", "--json"])
        packages = []
        try:
            data = json.loads(out)
            for name, info in data.get("venvs", {}).items():
                meta = info.get("metadata", {})
                pkg_meta = meta.get("main_package", {})
                installed = pkg_meta.get("package_version", "unknown")
                packages.append(
                    Package(manager="pipx", name=name, installed=installed, source="pipx")
                )
        except (json.JSONDecodeError, KeyError):
            pass
        return packages

    def list_outdated(self) -> list[Package]:
        # pipx has no native outdated json; skip
        return []

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["pipx", "upgrade", pkg]
        return ["pipx", "upgrade-all"]


class CargoAdapter(Adapter):
    name = "cargo"
    binary = "cargo"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["cargo", "install", "--list"])
        packages = []
        current_pkg = None
        for line in out.splitlines():
            if line and not line.startswith(" "):
                # "name version:"
                m = re.match(r"^(\S+)\s+v?(\S+?):", line)
                if m:
                    current_pkg = Package(
                        manager="cargo",
                        name=m.group(1),
                        installed=m.group(2),
                        source="cargo",
                    )
                    packages.append(current_pkg)
        return packages

    def list_outdated(self) -> list[Package]:
        if not _which("cargo-install-update"):
            return []
        rc, out, _ = _run(["cargo", "install-update", "-l"], timeout=120)
        packages = []
        for line in out.splitlines():
            # format: name  current  latest  needsUpdate
            parts = line.split()
            if len(parts) >= 4 and parts[3].lower() in ("yes", "no"):
                status = "outdated" if parts[3].lower() == "yes" else "current"
                packages.append(
                    Package(
                        manager="cargo",
                        name=parts[0],
                        installed=parts[1],
                        latest=parts[2],
                        status=status,
                        source="cargo",
                    )
                )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if _which("cargo-install-update"):
            if pkg:
                return ["cargo", "install-update", pkg]
            return ["cargo", "install-update", "-a"]
        if pkg:
            return ["cargo", "install", "--force", pkg]
        return ["cargo", "install-update", "-a"]  # will fail gracefully if missing


class RustupAdapter(Adapter):
    name = "rustup"
    binary = "rustup"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["rustup", "toolchain", "list"])
        packages = []
        for line in out.splitlines():
            line = line.strip()
            if line:
                # remove "(default)" etc
                name = line.split()[0]
                packages.append(
                    Package(manager="rustup", name=name, installed=name, source="rustup")
                )
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["rustup", "check"])
        packages = []
        for line in out.splitlines():
            # "stable-aarch64-apple-darwin - Update available : 1.x -> 1.y"
            if "Update available" in line:
                m = re.match(r"^(\S+)\s+-\s+Update available\s+:\s+(\S+)\s+->\s+(\S+)", line)
                if m:
                    packages.append(
                        Package(
                            manager="rustup",
                            name=m.group(1),
                            installed=m.group(2),
                            latest=m.group(3),
                            status="outdated",
                            source="rustup",
                        )
                    )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        return ["rustup", "update"]

    def self_update_cmd(self) -> list[str]:
        return ["rustup", "self", "update"]


class GemAdapter(Adapter):
    name = "gem"
    binary = "gem"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["gem", "list", "--no-versions"])
        packages = []
        # parse "gem list" with versions
        rc2, out2, _ = _run(["gem", "list"])
        for line in out2.splitlines():
            m = re.match(r"^(\S+)\s+\((.+)\)", line)
            if m:
                versions = m.group(2).split(", ")
                packages.append(
                    Package(
                        manager="gem",
                        name=m.group(1),
                        installed=versions[0],
                        source="gem",
                    )
                )
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["gem", "outdated"])
        packages = []
        for line in out.splitlines():
            # "name (installed < latest)"
            m = re.match(r"^(\S+)\s+\((\S+)\s+<\s+(\S+)\)", line)
            if m:
                packages.append(
                    Package(
                        manager="gem",
                        name=m.group(1),
                        installed=m.group(2),
                        latest=m.group(3),
                        status="outdated",
                        source="gem",
                    )
                )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["gem", "update", pkg]
        return ["gem", "update"]


class GoAdapter(Adapter):
    name = "go"
    binary = "go"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["go", "env", "GOPATH"])
        gopath = out.strip() or os.path.expanduser("~/go")
        bin_dir = Path(gopath) / "bin"
        packages = []
        if bin_dir.is_dir():
            for binary in sorted(bin_dir.iterdir()):
                if binary.is_file():
                    packages.append(
                        Package(
                            manager="go",
                            name=binary.name,
                            installed="unknown",
                            source="go-bin",
                        )
                    )
        return packages

    def list_outdated(self) -> list[Package]:
        return []  # go binaries have no standard version check

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["go", "install", f"{pkg}@latest"]
        return []  # no bulk upgrade possible without tracking import paths


class MiseAdapter(Adapter):
    name = "mise"
    binary = "mise"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["mise", "ls", "--current"])
        packages = []
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 2:
                packages.append(
                    Package(manager="mise", name=parts[0], installed=parts[1], source="mise")
                )
        return packages

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["mise", "outdated"])
        packages = []
        for line in out.splitlines():
            parts = line.split()
            if len(parts) >= 3 and not line.startswith("Plugin"):
                packages.append(
                    Package(
                        manager="mise",
                        name=parts[0],
                        installed=parts[1],
                        latest=parts[2],
                        status="outdated",
                        source="mise",
                    )
                )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["mise", "upgrade", pkg]
        return ["mise", "upgrade"]


class GhAdapter(Adapter):
    name = "gh"
    binary = "gh"

    def list_installed(self) -> list[Package]:
        rc, out, _ = _run(["gh", "extension", "list"])
        packages = []
        for line in out.splitlines():
            parts = line.split("\t")
            if len(parts) >= 2:
                packages.append(
                    Package(
                        manager="gh",
                        name=parts[0].strip(),
                        installed=parts[1].strip() if len(parts) > 2 else "unknown",
                        source="gh-ext",
                    )
                )
            elif len(parts) == 1 and parts[0].strip():
                packages.append(Package(manager="gh", name=parts[0].strip(), source="gh-ext"))
        return packages

    def list_outdated(self) -> list[Package]:
        return []  # gh has no outdated command; upgrade --all handles it

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        if pkg:
            return ["gh", "extension", "upgrade", pkg]
        return ["gh", "extension", "upgrade", "--all"]


class SoftwareUpdateAdapter(Adapter):
    """macOS softwareupdate — report only, never auto-apply."""

    name = "softwareupdate"
    binary = "softwareupdate"
    report_only = True  # excluded from `update --all`; needs explicit --manager + --apply

    def detect(self) -> bool:
        return sys.platform == "darwin" and _which("softwareupdate")

    def list_installed(self) -> list[Package]:
        return []  # no meaningful installed list

    def list_outdated(self) -> list[Package]:
        rc, out, _ = _run(["softwareupdate", "-l"], timeout=120)
        packages = []
        for line in out.splitlines():
            line = line.strip()
            if line.startswith("*") or line.startswith("-"):
                name = line.lstrip("*- ").strip()
                if name:
                    packages.append(
                        Package(
                            manager="softwareupdate",
                            name=name,
                            installed="installed",
                            latest="available",
                            status="outdated",
                            source="macos",
                        )
                    )
        return packages

    def upgrade_cmd(self, pkg: str | None = None) -> list[str]:
        # Report only — never called automatically
        return ["softwareupdate", "-ia", "--verbose"]

    def self_update_cmd(self) -> list[str]:
        return []


# ---------------------------------------------------------------------------
# Registry
# ---------------------------------------------------------------------------

ALL_ADAPTERS: list[Adapter] = [
    BrewAdapter(),
    NpmAdapter(),
    PnpmAdapter(),
    BunAdapter(),
    UvAdapter(),
    PipxAdapter(),
    CargoAdapter(),
    RustupAdapter(),
    GemAdapter(),
    GoAdapter(),
    MiseAdapter(),
    GhAdapter(),
    SoftwareUpdateAdapter(),
]


def live_adapters() -> list[Adapter]:
    return [a for a in ALL_ADAPTERS if a.detect()]


def get_adapter(name: str) -> Adapter | None:
    for a in ALL_ADAPTERS:
        if a.name == name:
            return a
    return None


# ---------------------------------------------------------------------------
# Database
# ---------------------------------------------------------------------------


def _db_connect() -> sqlite3.Connection:
    conn = sqlite3.connect(DB_PATH)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS tools (
            manager TEXT NOT NULL,
            name TEXT NOT NULL,
            installed TEXT NOT NULL DEFAULT '',
            latest TEXT,
            status TEXT,
            source TEXT,
            checked_at TEXT,
            -- installed is part of the key so multiple versions of one name (e.g. mise
            -- python 3.12/3.13/3.14) each persist instead of collapsing to one row.
            PRIMARY KEY (manager, name, installed)
        )
    """)
    conn.commit()
    return conn


def _upsert_packages(conn: sqlite3.Connection, packages: list[Package]) -> None:
    conn.executemany(
        """
        INSERT INTO tools (manager, name, installed, latest, status, source, checked_at)
        VALUES (:manager, :name, :installed, :latest, :status, :source, :checked_at)
        ON CONFLICT(manager, name, installed) DO UPDATE SET
            -- a re-scan (status='unknown', empty latest) must NOT wipe a prior check's
            -- result; only overwrite latest/status when the incoming row actually has one.
            latest=CASE WHEN excluded.latest IS NULL OR excluded.latest=''
                        THEN tools.latest ELSE excluded.latest END,
            status=CASE WHEN excluded.status='unknown' OR excluded.status IS NULL
                        THEN tools.status ELSE excluded.status END,
            source=excluded.source,
            checked_at=excluded.checked_at
        """,
        [
            {
                "manager": p.manager,
                "name": p.name,
                "installed": p.installed or "",
                "latest": p.latest,
                "status": p.status,
                "source": p.source,
                "checked_at": p.checked_at,
            }
            for p in packages
        ],
    )
    conn.commit()


def _load_all(conn: sqlite3.Connection) -> list[Package]:
    rows = conn.execute(
        "SELECT manager, name, installed, latest, status, source, checked_at FROM tools ORDER BY manager, name"
    ).fetchall()
    return [
        Package(
            manager=r[0],
            name=r[1],
            installed=r[2],
            latest=r[3],
            status=r[4],
            source=r[5],
            checked_at=r[6],
        )
        for r in rows
    ]


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------


def cmd_doctor(_args: argparse.Namespace) -> int:
    adapters = ALL_ADAPTERS
    print(f"\n{BOLD}pum doctor — adapter status{RESET}\n")
    print(f"  {'ADAPTER':<22}  {'BINARY':<22}  STATUS")
    print(f"  {'-' * 22}  {'-' * 22}  ------")
    for a in adapters:
        found = a.detect()
        status = _color("live", GREEN) if found else _color("not found", DIM)
        binary = shutil.which(a.binary) or a.binary
        print(f"  {a.name:<22}  {binary:<42}  {status}")
    print()
    return 0


def _scan_adapter(adapter: Adapter) -> tuple[str, list[Package], str | None]:
    try:
        pkgs = adapter.list_installed()
        return adapter.name, pkgs, None
    except Exception as exc:
        return adapter.name, [], str(exc)


def cmd_scan(_args: argparse.Namespace) -> int:
    adapters = live_adapters()
    if not adapters:
        print("No adapters detected.")
        return 1

    print(f"\n{BOLD}pum scan{RESET} — detecting packages …\n")
    all_packages: list[Package] = []
    errors: dict[str, str] = {}

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        futures = {pool.submit(_scan_adapter, a): a for a in adapters}
        for fut in concurrent.futures.as_completed(futures):
            name, pkgs, err = fut.result()
            if err:
                errors[name] = err
            else:
                all_packages.extend(pkgs)

    conn = _db_connect()
    _upsert_packages(conn, all_packages)

    # Summary table
    counts: dict[str, int] = {}
    for p in all_packages:
        counts[p.manager] = counts.get(p.manager, 0) + 1

    print(f"  {'MANAGER':<22}  {'PACKAGES':>8}")
    print(f"  {'-' * 22}  {'-' * 8}")
    total = 0
    for mgr, cnt in sorted(counts.items()):
        print(f"  {mgr:<22}  {cnt:>8}")
        total += cnt
    print(f"  {'TOTAL':<22}  {total:>8}")

    if errors:
        print(f"\n{YELLOW}Adapter errors:{RESET}")
        for name, err in errors.items():
            print(f"  {name}: {err}")

    # Write JSON
    JSON_PATH.write_text(
        json.dumps(
            [
                {
                    "manager": p.manager,
                    "name": p.name,
                    "installed": p.installed,
                    "latest": p.latest,
                    "status": p.status,
                    "source": p.source,
                    "checked_at": p.checked_at,
                }
                for p in all_packages
            ],
            indent=2,
        )
    )
    print(f"\n  DB  → {DB_PATH}")
    print(f"  JSON→ {JSON_PATH}\n")
    return 0


def _check_adapter(adapter: Adapter) -> tuple[str, list[Package], str | None]:
    try:
        pkgs = adapter.list_outdated()
        return adapter.name, pkgs, None
    except Exception as exc:
        return adapter.name, [], str(exc)


def cmd_check(_args: argparse.Namespace) -> int:
    adapters = live_adapters()
    if not adapters:
        print("No adapters detected.")
        return 1

    print(f"\n{BOLD}pum check{RESET} — querying outdated …\n")
    all_outdated: list[Package] = []
    errors: dict[str, str] = {}

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        futures = {pool.submit(_check_adapter, a): a for a in adapters}
        for fut in concurrent.futures.as_completed(futures):
            name, pkgs, err = fut.result()
            if err:
                errors[name] = err
            else:
                all_outdated.extend(pkgs)

    # Mark existing rows as current then upsert outdated
    conn = _db_connect()
    conn.execute(
        "UPDATE tools SET status='current', checked_at=? WHERE status='unknown'",
        (datetime.now(timezone.utc).isoformat(),),
    )
    conn.commit()
    _upsert_packages(conn, all_outdated)

    n = len(all_outdated)
    indicator = _color(str(n), RED if n > 0 else GREEN)
    print(f"  {indicator} update{'s' if n != 1 else ''} available\n")

    if errors:
        print(f"{YELLOW}Adapter errors:{RESET}")
        for name, err in errors.items():
            print(f"  {name}: {err}")
    return 0


def cmd_report(args: argparse.Namespace) -> int:
    conn = _db_connect()
    packages = _load_all(conn)
    total_in_db = len(packages)

    if args.outdated:
        packages = [p for p in packages if p.status == "outdated"]
    if hasattr(args, "manager") and args.manager:
        packages = [p for p in packages if p.manager == args.manager]

    if args.json:
        print(
            json.dumps(
                [
                    {
                        "manager": p.manager,
                        "name": p.name,
                        "installed": p.installed,
                        "latest": p.latest,
                        "status": p.status,
                        "source": p.source,
                    }
                    for p in packages
                ],
                indent=2,
            )
        )
        return 0

    if not packages:
        if total_in_db == 0:
            print("No packages in inventory. Run: pum scan")
        else:
            filt = []
            if args.outdated:
                filt.append("--outdated")
            if getattr(args, "manager", None):
                filt.append(f"--manager {args.manager}")
            print(
                f"No packages match {' '.join(filt) or 'the filter'} ({total_in_db} in inventory)."
            )
        return 0

    col_mgr = max(7, max(len(p.manager) for p in packages))
    col_pkg = max(7, max(len(p.name) for p in packages))
    col_inst = max(9, max(len(p.installed or "") for p in packages))
    col_lat = max(6, max(len(p.latest or "") for p in packages))

    header = (
        f"  {'MANAGER':<{col_mgr}}  {'PACKAGE':<{col_pkg}}  "
        f"{'INSTALLED':<{col_inst}}  {'LATEST':<{col_lat}}  STATUS"
    )
    sep = f"  {'-' * col_mgr}  {'-' * col_pkg}  {'-' * col_inst}  {'-' * col_lat}  ------"
    print(f"\n{BOLD}{header}{RESET}")
    print(sep)

    for p in packages:
        status_str = p.status or "unknown"
        if status_str == "outdated":
            status_col = _color(status_str, RED + BOLD)
            marker = _color("!", YELLOW)
        elif status_str == "current":
            status_col = _color(status_str, GREEN)
            marker = " "
        else:
            status_col = _color(status_str, DIM)
            marker = " "

        row = (
            f"{marker} {p.manager:<{col_mgr}}  {p.name:<{col_pkg}}  "
            f"{(p.installed or ''):<{col_inst}}  {(p.latest or ''):<{col_lat}}  {status_col}"
        )
        print(row)
    print()
    return 0


def cmd_update(args: argparse.Namespace) -> int:
    adapters = live_adapters()

    if args.manager:
        adapter = get_adapter(args.manager)
        if not adapter or not adapter.detect():
            print(f"Adapter '{args.manager}' not found or not installed.")
            return 1
        adapters = [adapter]

    # If specific packages given, find their owning adapter
    if args.packages:
        conn = _db_connect()
        for pkg_name in args.packages:
            rows = conn.execute(
                "SELECT manager FROM tools WHERE name=? ORDER BY checked_at DESC LIMIT 1",
                (pkg_name,),
            ).fetchall()
            if not rows:
                print(f"  {pkg_name}: not found in inventory (run pum scan first)")
                continue
            mgr_name = rows[0][0]
            adapter = get_adapter(mgr_name)
            if not adapter:
                print(f"  {pkg_name}: adapter '{mgr_name}' not in registry")
                continue
            argv = adapter.upgrade_cmd(pkg_name)
            if args.dry_run:
                print(f"  [dry-run] {' '.join(argv)}")
            else:
                print(f"  upgrading {pkg_name} via {mgr_name}: {' '.join(argv)}")
                rc, out, err = _run(argv, timeout=300)
                if rc != 0:
                    print(f"    {RED}error:{RESET} {err or out}")
                else:
                    print(f"    {GREEN}ok{RESET}")
        return 0

    # Bulk upgrade
    # Check if topgrade is available for --all
    if getattr(args, "all", False) and _which("topgrade"):
        argv = ["topgrade"]
        if args.dry_run:
            print(f"  [dry-run] {' '.join(argv)}")
        else:
            print("  running topgrade …")
            rc, out, err = _run(argv, timeout=600)
            print(out)
            if rc != 0:
                print(f"{RED}topgrade error:{RESET} {err}")
        return 0

    for adapter in adapters:
        # report-only adapters (softwareupdate) are NEVER run by --all; they need an
        # explicit `pum update --manager <name> --apply` to guard against OS-update/reboot.
        if adapter.report_only:
            targeted = args.manager == adapter.name
            if not (targeted and getattr(args, "apply", False)):
                print(
                    f"  [{adapter.name}] {YELLOW}skipped — report-only{RESET}. "
                    f"Run: pum update --manager {adapter.name} --apply"
                )
                continue
        argv = adapter.upgrade_cmd()
        if not argv:
            continue
        if args.dry_run:
            print(f"  [dry-run] [{adapter.name}] {' '.join(argv)}")
        else:
            print(f"  [{adapter.name}] {' '.join(argv)} …")
            rc, out, err = _run(argv, timeout=300)
            if rc != 0:
                print(f"    {RED}error:{RESET} {err or out[:200]}")
            else:
                print(f"    {GREEN}ok{RESET}")
    return 0


def cmd_self(args: argparse.Namespace) -> int:
    self_adapters = [
        a for a in [BrewAdapter(), RustupAdapter(), UvAdapter(), BunAdapter()] if a.detect()
    ]
    print(f"\n{BOLD}pum self{RESET} — manager self-update status\n")
    for adapter in self_adapters:
        argv = adapter.self_update_cmd()
        if not argv:
            continue
        if args.apply:
            print(f"  [{adapter.name}] {' '.join(argv)} …")
            rc, out, err = _run(argv, timeout=300)
            if rc != 0:
                print(f"    {RED}error:{RESET} {err or out[:200]}")
            else:
                print(f"    {GREEN}ok{RESET}")
        else:
            print(f"  [{adapter.name}] would run: {' '.join(argv)}")
    if not args.apply:
        print("\n  Run with --apply to execute.\n")
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="pum",
        description="Package Update Manager — multi-manager inventory and updates",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("scan", help="Detect managers, list installed packages, write DB+JSON")
    sub.add_parser("check", help="Query outdated packages across all managers")

    report = sub.add_parser("report", help="Print inventory table")
    report.add_argument("--json", action="store_true", help="Output as JSON")
    report.add_argument("--outdated", action="store_true", help="Show only outdated packages")
    report.add_argument("--manager", "-m", metavar="M", help="Filter by manager name")

    update = sub.add_parser("update", help="Upgrade packages")
    update.add_argument("packages", nargs="*", metavar="pkg", help="Specific packages to upgrade")
    update.add_argument("--manager", "-m", metavar="M", help="Restrict to this manager")
    update.add_argument(
        "--all",
        action="store_true",
        help="Upgrade everything (uses topgrade if available)",
    )
    update.add_argument("--dry-run", action="store_true", help="Print commands without executing")
    update.add_argument(
        "--apply",
        action="store_true",
        help="Required to actually run report-only managers (e.g. softwareupdate)",
    )

    self_cmd = sub.add_parser("self", help="Check/update the managers themselves")
    self_cmd.add_argument("--apply", action="store_true", help="Execute the self-update commands")

    sub.add_parser("doctor", help="Show which adapters are live on this host")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    dispatch = {
        "scan": cmd_scan,
        "check": cmd_check,
        "report": cmd_report,
        "update": cmd_update,
        "self": cmd_self,
        "doctor": cmd_doctor,
    }
    fn = dispatch.get(args.command)
    if fn is None:
        parser.print_help()
        return 1
    return fn(args)


if __name__ == "__main__":
    sys.exit(main())
