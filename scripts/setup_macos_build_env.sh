#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must be run on macOS."
  exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode command line tools are missing."
  echo "Install Xcode (or CLI tools) first, then rerun this script."
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required but not found."
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup is still unavailable after installation attempt."
  exit 1
fi

rustup toolchain install stable --profile minimal
rustup default stable
rustup target add aarch64-apple-darwin
rustup component add rustfmt clippy

if ! grep -q '.cargo/env' "$HOME/.zprofile" 2>/dev/null; then
  printf '\nsource "$HOME/.cargo/env"\n' >> "$HOME/.zprofile"
fi

echo "macOS build environment ready."
rustc --version
cargo --version
