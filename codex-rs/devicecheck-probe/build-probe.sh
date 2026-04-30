#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage: build-probe.sh --target TARGET --out DIR

Builds DeviceCheckProbe.app for the requested macOS Rust target triple.
The caller is responsible for code signing the resulting app bundle.
USAGE
}

target=""
out_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      target="${2:-}"
      shift 2
      ;;
    --out)
      out_dir="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$target" || -z "$out_dir" ]]; then
  usage
  exit 2
fi

case "$target" in
  aarch64-apple-darwin)
    swift_target="arm64-apple-macosx13.0"
    ;;
  x86_64-apple-darwin)
    swift_target="x86_64-apple-macosx13.0"
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 2
    ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app_dir="$out_dir/DeviceCheckProbe.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
module_cache_path="$out_dir/module-cache"

rm -rf "$app_dir"
mkdir -p "$macos_dir" "$module_cache_path"
cp "$script_dir/Info.plist" "$contents_dir/Info.plist"

CLANG_MODULE_CACHE_PATH="$module_cache_path" \
MACOSX_DEPLOYMENT_TARGET=13.0 \
swiftc \
  -target "$swift_target" \
  -framework DeviceCheck \
  -framework Foundation \
  "$script_dir/DeviceCheckProbe.swift" \
  -o "$macos_dir/DeviceCheckProbe"

echo "$app_dir"
