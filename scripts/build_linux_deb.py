#!/usr/bin/env python3
"""
Build a Debian package for PDF Markdown Studio.

The resulting .deb installs:
- /opt/pdf-markdown-studio/*
- /usr/bin/pdf-markdown-studio (launcher)
- /usr/share/applications/pdf-markdown-studio.desktop
- /usr/share/icons/hicolor/512x512/apps/pdf-markdown-studio.png
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import tomllib
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--version", default="")
    return parser.parse_args()


def debianize_version(version: str) -> str:
    return version.replace("-", "~")


def read_version(repo_root: Path) -> str:
    cargo_toml = repo_root / "Cargo.toml"
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    return str(data["package"]["version"]).strip()


def write_text(path: Path, content: str, mode: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="\n")
    if mode is not None:
        os.chmod(path, mode)


def copy_file(src: Path, dst: Path, mode: int | None = None) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)
    if mode is not None:
        os.chmod(dst, mode)


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    binary_path = Path(args.binary).resolve()
    output_dir = Path(args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    if not binary_path.exists():
        raise RuntimeError(f"binary not found: {binary_path}")

    version = args.version.strip() or read_version(repo_root)
    deb_version = debianize_version(version)

    package_name = "pdf-markdown-studio"
    package_dir = output_dir / "deb-stage"
    if package_dir.exists():
        shutil.rmtree(package_dir)

    debian_dir = package_dir / "DEBIAN"
    opt_root = package_dir / "opt" / package_name
    bin_dir = package_dir / "usr" / "bin"
    applications_dir = package_dir / "usr" / "share" / "applications"
    icon_dir = package_dir / "usr" / "share" / "icons" / "hicolor" / "512x512" / "apps"

    for path in (debian_dir, opt_root, bin_dir, applications_dir, icon_dir):
        path.mkdir(parents=True, exist_ok=True)

    control = f"""Package: {package_name}
Version: {deb_version}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: OpenResearchTools
Depends: libc6, libgcc-s1, libstdc++6, libgl1, libx11-6, libxkbcommon0, libwayland-client0, libasound2, libgtk-3-0
Description: PDF Markdown Studio desktop app
 Side-by-side PDF/Image and Markdown workflow with OpenResearchTools runtime integration.
"""
    write_text(debian_dir / "control", control)

    postinst = """#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
"""
    postrm = """#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
"""
    write_text(debian_dir / "postinst", postinst, mode=0o755)
    write_text(debian_dir / "postrm", postrm, mode=0o755)

    launcher = """#!/bin/sh
exec /opt/pdf-markdown-studio/pdf_markdown_studio "$@"
"""
    write_text(bin_dir / package_name, launcher, mode=0o755)

    copy_file(binary_path, opt_root / "pdf_markdown_studio", mode=0o755)
    copy_file(
        repo_root / "packaging" / "linux" / "pdf-markdown-studio.desktop",
        applications_dir / "pdf-markdown-studio.desktop",
        mode=0o644,
    )
    copy_file(
        repo_root / "logo" / "linux" / "pdf-markdown-studio.png",
        icon_dir / "pdf-markdown-studio.png",
        mode=0o644,
    )
    deb_path = output_dir / f"{package_name}_{deb_version}_amd64.deb"
    if deb_path.exists():
        deb_path.unlink()

    subprocess.run(
        [
            "dpkg-deb",
            "--build",
            "--root-owner-group",
            str(package_dir),
            str(deb_path),
        ],
        check=True,
    )

    print(f"Built package: {deb_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
