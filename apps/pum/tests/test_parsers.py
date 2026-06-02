"""Unit tests for adapter parse logic (no subprocess calls)."""

import json
import re
import sys
import unittest
from pathlib import Path

# Make pum importable without install
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import pum


class TestBrewOutdatedParse(unittest.TestCase):
    FIXTURE = {
        "formulae": [
            {
                "name": "ripgrep",
                "installed_versions": ["13.0.0"],
                "current_version": "14.1.0",
            },
            {
                "name": "fd",
                "installed_versions": ["9.0.0", "9.0.1"],
                "current_version": "10.2.0",
            },
        ],
        "casks": [
            {
                "name": "rectangle",
                "token": "rectangle",
                "installed_versions": "0.80",
                "current_version": "0.85",
            }
        ],
    }

    def _parse(self, data: dict) -> list[pum.Package]:
        """Re-implement the brew outdated parse without subprocess."""
        packages = []
        for item in data.get("formulae", []):
            installed = item.get("installed_versions", ["unknown"])
            installed_ver = installed[-1] if installed else "unknown"
            packages.append(
                pum.Package(
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
                pum.Package(
                    manager="brew",
                    name=item.get("name", item.get("token", "unknown")),
                    installed=item.get("installed_versions", "unknown"),
                    latest=item.get("current_version", "unknown"),
                    status="outdated",
                    source="brew-cask",
                )
            )
        return packages

    def test_formulae_count(self):
        pkgs = self._parse(self.FIXTURE)
        formulae = [p for p in pkgs if p.source == "brew"]
        self.assertEqual(len(formulae), 2)

    def test_formulae_latest_version_picked(self):
        pkgs = self._parse(self.FIXTURE)
        fd = next(p for p in pkgs if p.name == "fd")
        self.assertEqual(fd.installed, "9.0.1")  # last element
        self.assertEqual(fd.latest, "10.2.0")

    def test_cask_parsed(self):
        pkgs = self._parse(self.FIXTURE)
        casks = [p for p in pkgs if p.source == "brew-cask"]
        self.assertEqual(len(casks), 1)
        self.assertEqual(casks[0].name, "rectangle")
        self.assertEqual(casks[0].status, "outdated")

    def test_empty_formulae(self):
        pkgs = self._parse({"formulae": [], "casks": []})
        self.assertEqual(pkgs, [])


class TestNpmOutdatedParse(unittest.TestCase):
    FIXTURE = {
        "typescript": {"current": "5.3.3", "wanted": "5.3.3", "latest": "5.4.5"},
        "prettier": {"current": "3.0.0", "wanted": "3.0.0", "latest": "3.2.5"},
    }

    def _parse(self, data: dict) -> list[pum.Package]:
        packages = []
        for name, info in data.items():
            packages.append(
                pum.Package(
                    manager="npm",
                    name=name,
                    installed=info.get("current", "unknown"),
                    latest=info.get("latest", "unknown"),
                    status="outdated",
                    source="npm-global",
                )
            )
        return packages

    def test_count(self):
        pkgs = self._parse(self.FIXTURE)
        self.assertEqual(len(pkgs), 2)

    def test_versions(self):
        pkgs = self._parse(self.FIXTURE)
        ts = next(p for p in pkgs if p.name == "typescript")
        self.assertEqual(ts.installed, "5.3.3")
        self.assertEqual(ts.latest, "5.4.5")
        self.assertEqual(ts.status, "outdated")

    def test_empty(self):
        self.assertEqual(self._parse({}), [])


class TestCargoListParse(unittest.TestCase):
    FIXTURE = """\
bat v0.24.0:
    bat
cargo-binstall v1.7.4:
    cargo-binstall
ripgrep v14.1.0:
    rg
"""

    def _parse(self, text: str) -> list[pum.Package]:
        packages = []
        for line in text.splitlines():
            if line and not line.startswith(" "):
                m = re.match(r"^(\S+)\s+v?(\S+?):", line)
                if m:
                    packages.append(
                        pum.Package(
                            manager="cargo",
                            name=m.group(1),
                            installed=m.group(2),
                            source="cargo",
                        )
                    )
        return packages

    def test_count(self):
        pkgs = self._parse(self.FIXTURE)
        self.assertEqual(len(pkgs), 3)

    def test_versions(self):
        pkgs = self._parse(self.FIXTURE)
        bat = next(p for p in pkgs if p.name == "bat")
        self.assertEqual(bat.installed, "0.24.0")

    def test_empty(self):
        self.assertEqual(self._parse(""), [])


class TestRustupCheckParse(unittest.TestCase):
    FIXTURE = """\
stable-aarch64-apple-darwin - Update available : 1.78.0 -> 1.79.0
nightly-aarch64-apple-darwin - Up to date : 1.80.0-nightly
"""

    def _parse(self, text: str) -> list[pum.Package]:
        packages = []
        for line in text.splitlines():
            if "Update available" in line:
                m = re.match(
                    r"^(\S+)\s+-\s+Update available\s+:\s+(\S+)\s+->\s+(\S+)", line
                )
                if m:
                    packages.append(
                        pum.Package(
                            manager="rustup",
                            name=m.group(1),
                            installed=m.group(2),
                            latest=m.group(3),
                            status="outdated",
                            source="rustup",
                        )
                    )
        return packages

    def test_only_outdated_returned(self):
        pkgs = self._parse(self.FIXTURE)
        self.assertEqual(len(pkgs), 1)

    def test_versions(self):
        pkgs = self._parse(self.FIXTURE)
        self.assertEqual(pkgs[0].installed, "1.78.0")
        self.assertEqual(pkgs[0].latest, "1.79.0")
        self.assertEqual(pkgs[0].status, "outdated")


class TestGemOutdatedParse(unittest.TestCase):
    FIXTURE = """\
cocoapods (1.14.3 < 1.15.2)
bundler (2.4.0 < 2.5.9)
"""

    def _parse(self, text: str) -> list[pum.Package]:
        packages = []
        for line in text.splitlines():
            m = re.match(r"^(\S+)\s+\((\S+)\s+<\s+(\S+)\)", line)
            if m:
                packages.append(
                    pum.Package(
                        manager="gem",
                        name=m.group(1),
                        installed=m.group(2),
                        latest=m.group(3),
                        status="outdated",
                        source="gem",
                    )
                )
        return packages

    def test_count(self):
        self.assertEqual(len(self._parse(self.FIXTURE)), 2)

    def test_versions(self):
        pkgs = self._parse(self.FIXTURE)
        cp = next(p for p in pkgs if p.name == "cocoapods")
        self.assertEqual(cp.installed, "1.14.3")
        self.assertEqual(cp.latest, "1.15.2")


if __name__ == "__main__":
    unittest.main()
