# macOS Packaging Assets

Optional app icon path for local/CI macOS packaging:

- `logo/macos/AppIcon.icns`

If this file exists, `scripts/build_macos_dmg.sh` and the GitHub Actions macOS job include it in the `.app` bundle automatically.
