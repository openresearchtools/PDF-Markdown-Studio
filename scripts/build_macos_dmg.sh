#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must be run on macOS."
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="aarch64-apple-darwin"
profile="release"
binary_name="pdf_markdown_studio"
binary_path=""
output_dir=""
version=""
app_name="PDF Markdown Studio"
bundle_id="tools.openresearch.pdfmarkdownstudio"
icon_path=""
skip_build=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-root)
      repo_root="$2"
      shift 2
      ;;
    --target)
      target="$2"
      shift 2
      ;;
    --profile)
      profile="$2"
      shift 2
      ;;
    --binary-name)
      binary_name="$2"
      shift 2
      ;;
    --binary)
      binary_path="$2"
      shift 2
      ;;
    --output-dir)
      output_dir="$2"
      shift 2
      ;;
    --version)
      version="$2"
      shift 2
      ;;
    --app-name)
      app_name="$2"
      shift 2
      ;;
    --bundle-id)
      bundle_id="$2"
      shift 2
      ;;
    --icon)
      icon_path="$2"
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    *)
      echo "Unknown argument: $1"
      exit 1
      ;;
  esac
done

repo_root="$(cd "$repo_root" && pwd)"
output_dir="${output_dir:-$repo_root/dist}"
mkdir -p "$output_dir"

if [[ -z "$version" ]]; then
  version="$(awk -F'"' '/^version = "/ { print $2; exit }' "$repo_root/Cargo.toml")"
fi

if [[ -z "$binary_path" ]]; then
  binary_path="$repo_root/target/$target/$profile/$binary_name"
fi
if [[ -z "$icon_path" ]]; then
  default_icon="$repo_root/logo/macos/AppIcon.icns"
  if [[ -f "$default_icon" ]]; then
    icon_path="$default_icon"
  fi
fi

if [[ "$skip_build" -eq 0 ]]; then
  cargo_args=(build --locked --target "$target")
  if [[ "$profile" == "release" ]]; then
    cargo_args+=(--release)
  fi
  (cd "$repo_root" && cargo "${cargo_args[@]}")
fi

if [[ ! -f "$binary_path" ]]; then
  echo "Built binary not found: $binary_path"
  exit 1
fi

app_bundle="$output_dir/$app_name.app"
contents_dir="$app_bundle/Contents"
macos_dir="$contents_dir/MacOS"
resources_dir="$contents_dir/Resources"
stage_root="$output_dir/macos-stage"
dmg_root="$stage_root/dmg-root"
dmg_path="$output_dir/pdf-markdown-studio-${version}-macos-arm64.dmg"

rm -rf "$app_bundle" "$stage_root"
mkdir -p "$macos_dir" "$resources_dir" "$dmg_root"

cp "$binary_path" "$macos_dir/$binary_name"
chmod 755 "$macos_dir/$binary_name"

icon_plist_block=""
if [[ -n "$icon_path" ]]; then
  if [[ ! -f "$icon_path" ]]; then
    echo "Icon file not found: $icon_path"
    exit 1
  fi
  cp "$icon_path" "$resources_dir/AppIcon.icns"
  icon_plist_block=$'    <key>CFBundleIconFile</key>\n    <string>AppIcon</string>'
fi

cat > "$contents_dir/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${app_name}</string>
    <key>CFBundleDisplayName</key>
    <string>${app_name}</string>
    <key>CFBundleIdentifier</key>
    <string>${bundle_id}</string>
    <key>CFBundleExecutable</key>
    <string>${binary_name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
${icon_plist_block}
</dict>
</plist>
EOF

cp -R "$app_bundle" "$dmg_root/"
ln -s /Applications "$dmg_root/Applications"

rm -f "$dmg_path"
hdiutil create \
  -volname "$app_name" \
  -srcfolder "$dmg_root" \
  -ov \
  -format UDZO \
  "$dmg_path"

echo "Built app bundle: $app_bundle"
echo "Built dmg: $dmg_path"
