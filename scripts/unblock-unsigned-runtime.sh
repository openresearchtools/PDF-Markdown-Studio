#!/usr/bin/env sh
set -eu

runtime_dir="${1:-}"
if [ -z "$runtime_dir" ]; then
  echo "Runtime path argument is required." >&2
  exit 2
fi
if [ ! -d "$runtime_dir" ]; then
  echo "Runtime directory does not exist: $runtime_dir" >&2
  exit 2
fi

# macOS: clear security xattrs recursively when present.
if command -v xattr >/dev/null 2>&1; then
  xattr -dr com.apple.quarantine "$runtime_dir" >/dev/null 2>&1 || true
  xattr -dr com.apple.provenance "$runtime_dir" >/dev/null 2>&1 || true
fi

# macOS: normalize copied Mach-O install names/rpaths so dylibs loaded from the
# app-data runtime resolve siblings from that same runtime folder instead of
# stale Cargo build locations.
if command -v install_name_tool >/dev/null 2>&1 && command -v otool >/dev/null 2>&1; then
  find "$runtime_dir" -maxdepth 1 -type f \( \
    -name "Engine" -o -name "example-cli" -o -name "rpc-server" -o \
    -name "*.dylib" \
  \) | while IFS= read -r file; do
    case "$file" in
      *.dylib)
        install_name_tool -id "@loader_path/$(basename "$file")" "$file" >/dev/null 2>&1 || true
        ;;
    esac
    if ! otool -l "$file" | awk '
        $1 == "cmd" && $2 == "LC_RPATH" { flag = 1; next }
        flag && $1 == "path" { print $2; flag = 0 }
      ' | grep -Fxq "@loader_path"; then
      install_name_tool -add_rpath "@loader_path" "$file" >/dev/null 2>&1 || true
    fi
  done
fi

# Ensure runtime binaries/scripts are executable where relevant.
if command -v find >/dev/null 2>&1; then
  find "$runtime_dir" -type f \( \
    -name "Engine" -o -name "example-cli" -o -name "rpc-server" -o \
    -name "ffmpeg" -o -name "ffprobe" -o -name "*.sh" -o \
    -name "*.dylib" -o -name "*.so" -o -name "*.so.*" \
  \) -exec chmod +x {} \; >/dev/null 2>&1 || true
fi

# macOS: ad-hoc sign runtime Mach-O binaries so local dylib loads stop tripping
# over unsigned runtime checks after copy/download into app data.
if command -v codesign >/dev/null 2>&1; then
  find "$runtime_dir" -type f \( \
    -name "Engine" -o -name "example-cli" -o -name "rpc-server" -o \
    -name "ffmpeg" -o -name "ffprobe" -o \
    -name "*.dylib" -o -name "*.so" -o -name "*.so.*" \
  \) -exec codesign --force --sign - --timestamp=none {} \; >/dev/null 2>&1 || true
fi

echo "Unsigned runtime unblock complete for '$runtime_dir'."
