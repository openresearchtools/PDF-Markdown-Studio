#!/usr/bin/env python3
"""
Resolve (and optionally fetch/extract) the correct engine runtime asset from manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path
from typing import Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True, help="Path to engine manifest JSON")
    parser.add_argument("--platform", required=True, help="Platform key, e.g. windows-x64")
    parser.add_argument(
        "--backend",
        default="auto",
        help="Backend override (vulkan/metal/cuda/...) or 'auto'",
    )
    parser.add_argument("--output-dir", help="Output runtime directory (fetch mode)")
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="Only validate/resolve manifest mapping, do not download runtime",
    )
    parser.add_argument(
        "--selection-out",
        help="Optional path to write selected asset metadata JSON",
    )
    return parser.parse_args()


def load_manifest(path: Path) -> Dict:
    raw = path.read_text(encoding="utf-8")
    parsed = json.loads(raw)
    if not isinstance(parsed, dict) or not isinstance(parsed.get("assets"), list):
        raise ValueError(f"invalid manifest format: {path}")
    return parsed


def backend_priority(platform: str, backend: str) -> int:
    b = backend.strip().lower()
    if platform == "windows-x64":
        if b == "vulkan":
            return 500
        if b == "cuda":
            return 400
        return 100
    if platform == "macos-arm64":
        if b == "metal":
            return 500
        return 100
    if platform == "ubuntu-x64":
        if b == "vulkan":
            return 500
        return 100
    return 0


def select_asset(manifest: Dict, platform: str, backend: str) -> Dict:
    assets: List[Dict] = [
        a
        for a in manifest.get("assets", [])
        if str(a.get("platform", "")).strip().lower() == platform.strip().lower()
    ]
    if not assets:
        raise RuntimeError(f"no runtime assets found for platform '{platform}'")

    if backend.strip().lower() != "auto":
        filtered = [
            a
            for a in assets
            if str(a.get("backend", "")).strip().lower() == backend.strip().lower()
        ]
        if not filtered:
            raise RuntimeError(
                f"no runtime assets found for platform '{platform}' and backend '{backend}'"
            )
        assets = filtered

    assets.sort(
        key=lambda a: backend_priority(
            platform, str(a.get("backend", "")).strip().lower()
        ),
        reverse=True,
    )
    return assets[0]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        while True:
            chunk = fh.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def download_file(url: str, destination: Path) -> None:
    req = urllib.request.Request(url, headers={"User-Agent": "pdf-markdown-studio-ci"})
    with urllib.request.urlopen(req, timeout=120) as response:
        with destination.open("wb") as out:
            shutil.copyfileobj(response, out)


def extract_archive(archive_path: Path, archive_type: str, output_dir: Path) -> None:
    if archive_type == "zip" or archive_path.name.lower().endswith(".zip"):
        with zipfile.ZipFile(archive_path) as zf:
            zf.extractall(output_dir)
        return

    if archive_type == "tar.gz" or archive_path.name.lower().endswith(".tar.gz"):
        with tarfile.open(archive_path, "r:gz") as tf:
            tf.extractall(output_dir)
        return

    raise RuntimeError(
        f"unsupported archive type '{archive_type}' for '{archive_path.name}'"
    )


def is_runtime_root(path: Path) -> bool:
    for marker in (
        "llama-server-bridge.dll",
        "libllama-server-bridge.dylib",
        "libllama-server-bridge.so",
    ):
        if (path / marker).exists():
            return True
    return False


def locate_runtime_root(extract_dir: Path) -> Path:
    if is_runtime_root(extract_dir):
        return extract_dir

    subdirs = [p for p in extract_dir.iterdir() if p.is_dir()]
    if len(subdirs) == 1:
        if is_runtime_root(subdirs[0]):
            return subdirs[0]
        # Fallback: many runtime archives have a single top-level folder.
        return subdirs[0]

    return extract_dir


def copy_tree(src: Path, dst: Path) -> None:
    dst.mkdir(parents=True, exist_ok=True)
    for item in src.iterdir():
        target = dst / item.name
        if item.is_dir():
            shutil.copytree(item, target, dirs_exist_ok=True)
        else:
            shutil.copy2(item, target)


def main() -> int:
    args = parse_args()

    manifest_path = Path(args.manifest).resolve()

    manifest = load_manifest(manifest_path)
    selected = select_asset(manifest, args.platform, args.backend)

    asset_url = str(selected.get("url", "")).strip()
    archive_name = str(selected.get("file_name", "")).strip() or "engine-runtime"
    archive_type = str(selected.get("archive", "")).strip().lower() or "zip"
    expected_sha = str(selected.get("sha256", "")).strip().lower()
    manifest_tag = str(manifest.get("tag", "")).strip()
    selected_backend = str(selected.get("backend", "")).strip()

    if not asset_url:
        raise RuntimeError("selected runtime asset has empty URL")

    print(f"Manifest: {manifest_path}")
    print(f"Manifest tag: {manifest_tag}")
    print(f"Selected platform: {args.platform}")
    print(f"Selected backend: {selected_backend}")
    print(f"Selected asset: {archive_name}")
    print(f"Selected URL: {asset_url}")

    metadata = {
        "manifest_tag": manifest_tag,
        "platform": args.platform,
        "backend": selected_backend,
        "asset_file_name": archive_name,
        "asset_url": asset_url,
    }

    if args.selection_out:
        selection_out_path = Path(args.selection_out).resolve()
        selection_out_path.parent.mkdir(parents=True, exist_ok=True)
        selection_out_path.write_text(
            json.dumps(metadata, indent=2) + os.linesep,
            encoding="utf-8",
        )
        print(f"Selection metadata written to: {selection_out_path}")

    if args.check_only:
        return 0

    if not args.output_dir:
        raise RuntimeError("--output-dir is required unless --check-only is used")

    output_dir = Path(args.output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Downloading: {asset_url}")

    with tempfile.TemporaryDirectory(prefix="engine-runtime-ci-") as tmp:
        tmpdir = Path(tmp)
        archive_path = tmpdir / archive_name
        extract_dir = tmpdir / "extract"
        extract_dir.mkdir(parents=True, exist_ok=True)

        download_file(asset_url, archive_path)

        if expected_sha:
            actual_sha = sha256_file(archive_path).lower()
            if actual_sha != expected_sha:
                raise RuntimeError(
                    f"sha256 mismatch for {archive_name}: expected {expected_sha}, got {actual_sha}"
                )
            print(f"SHA256 verified: {actual_sha}")

        extract_archive(archive_path, archive_type, extract_dir)
        runtime_root = locate_runtime_root(extract_dir)

        if output_dir.exists():
            shutil.rmtree(output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        copy_tree(runtime_root, output_dir)

    (output_dir / "engine-runtime-ci-metadata.json").write_text(
        json.dumps(metadata, indent=2) + os.linesep,
        encoding="utf-8",
    )
    print(f"Runtime extracted to: {output_dir}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:  # pragma: no cover - CI script entrypoint
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1)
