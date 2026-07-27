#!/usr/bin/env python3
from __future__ import annotations

import argparse
import email.utils
import re
from datetime import datetime
from pathlib import Path

PROJECT_PACKAGES = {
    "we-layerd",
    "we-core",
    "we-gui",
    "we-renderer",
    "we-renderer-sys",
    "xtask",
}

MANIFESTS = (
    "Cargo.toml",
    "apps/we-gui/Cargo.toml",
    "crates/we-core/Cargo.toml",
    "crates/we-renderer-sys/Cargo.toml",
    "crates/we-renderer/Cargo.toml",
    "xtask/Cargo.toml",
)

VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def replace_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise RuntimeError(f"expected one version field in {path}, found {count}")
    path.write_text(updated, encoding="utf-8")


def update_lockfile(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    package_re = re.compile(r"(?ms)^\[\[package\]\]\n.*?(?=^\[\[package\]\]\n|\Z)")
    found: set[str] = set()

    def update_block(match: re.Match[str]) -> str:
        block = match.group(0)
        name_match = re.search(r'^name = "([^"]+)"$', block, flags=re.MULTILINE)
        if name_match is None or name_match.group(1) not in PROJECT_PACKAGES:
            return block

        name = name_match.group(1)
        updated, count = re.subn(
            r'^version = "[^"]+"$',
            f'version = "{version}"',
            block,
            count=1,
            flags=re.MULTILINE,
        )
        if count != 1:
            raise RuntimeError(f"expected one Cargo.lock version for {name}, found {count}")
        found.add(name)
        return updated

    updated = package_re.sub(update_block, text)
    missing = PROJECT_PACKAGES - found
    if missing:
        raise RuntimeError(f"missing workspace packages in Cargo.lock: {', '.join(sorted(missing))}")
    path.write_text(updated, encoding="utf-8")


def prepend_debian_changelog(path: Path, version: str, now: datetime) -> None:
    text = path.read_text(encoding="utf-8")
    top_match = re.match(r"we-layerd \(([^)]+)-1\) ", text)
    if top_match is not None and top_match.group(1) == version:
        return

    timestamp = email.utils.format_datetime(now)
    entry = (
        f"we-layerd ({version}-1) unstable; urgency=medium\n\n"
        f"  * Prepare {version}.\n\n"
        f" -- we-layerd contributors <noreply@example.invalid>  {timestamp}\n\n"
    )
    path.write_text(entry + text, encoding="utf-8")


def prepend_fedora_changelog(path: Path, version: str, now: datetime) -> None:
    text = path.read_text(encoding="utf-8")
    marker = f" - {version}-1\n"
    if marker in text:
        return

    weekdays = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
    months = (
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    )
    date = f"{weekdays[now.weekday()]} {months[now.month - 1]} {now.day:02d} {now.year}"
    entry = (
        f"* {date} Aromatic05 <noreply@example.invalid> - {version}-1\n"
        f"- Prepare {version}\n\n"
    )
    marker_index = text.find("%changelog\n")
    if marker_index < 0:
        raise RuntimeError(f"missing %changelog section in {path}")
    insert_at = marker_index + len("%changelog\n")
    path.write_text(text[:insert_at] + entry + text[insert_at:], encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description="Update all we-layerd version fields")
    parser.add_argument("version", help="new MAJOR.MINOR.PATCH version")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root used for testing",
    )
    args = parser.parse_args()

    if VERSION_RE.fullmatch(args.version) is None:
        parser.error(f"invalid version: {args.version}")

    root = args.repo_root.resolve()
    root_manifest = root / "Cargo.toml"
    current_match = re.search(
        r'^version = "([^"]+)"$',
        root_manifest.read_text(encoding="utf-8"),
        flags=re.MULTILINE,
    )
    if current_match is None:
        raise RuntimeError("failed to read current version from Cargo.toml")
    current_version = current_match.group(1)

    for manifest in MANIFESTS:
        replace_once(
            root / manifest,
            r'^version = "[^"]+"$',
            f'version = "{args.version}"',
        )

    update_lockfile(root / "Cargo.lock", args.version)
    replace_once(root / "package/archlinux/PKGBUILD", r"^pkgver=.*$", f"pkgver={args.version}")
    replace_once(
        root / "package/fedora/we-layerd.spec",
        r"^Version:\s+.*$",
        f"Version:        {args.version}",
    )

    now = datetime.now().astimezone()
    prepend_fedora_changelog(root / "package/fedora/we-layerd.spec", args.version, now)
    prepend_debian_changelog(root / "package/ubuntu/debian/changelog", args.version, now)
    print(f"updated version {current_version} -> {args.version}")


if __name__ == "__main__":
    main()
