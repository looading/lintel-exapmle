#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
destination="${1:-$root/target/macos/世界时钟.app}"
if [[ -e "$destination" || -L "$destination" ]]; then
    printf '已存在应用包 %s，请指定一个新的输出路径。\n' "$destination" >&2
    exit 1
fi
identity="${CLOCK_SIGN_IDENTITY:--}"
binary="${CLOCK_BINARY:-$root/target/debug/lintel-world-clock}"
if [[ -z "${CLOCK_BINARY:-}" ]]; then
    cargo build --locked
fi
[[ -x "$binary" ]] || { printf '缺少应用二进制 %s\n' "$binary" >&2; exit 1; }
if ! command -v xcodegen >/dev/null; then
    printf '生成原生扩展工程需要 XcodeGen 2.47 或更高版本。\n' >&2
    exit 1
fi

mkdir -p "$(dirname "$destination")"
stage="$(mktemp -d "$(dirname "$destination")/.clock-build.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
cp macos/{project.yml,ClockCity.swift,ClockWidget.swift,ClockIntents.swift} "$stage/"
xcodegen generate --spec "$stage/project.yml"
xcodebuild -project "$stage/ClockExtensions.xcodeproj" -scheme ClockExtensions \
    -configuration Release -destination 'generic/platform=macOS' \
    -derivedDataPath "$stage/build" -quiet build

app="$stage/世界时钟.app"
contents="$app/Contents"
products="$stage/build/Build/Products/Release"
mkdir -p "$contents/MacOS" "$contents/PlugIns" "$contents/Extensions"
cp "$binary" "$contents/MacOS/lintel-world-clock"
cp macos/Host-Info.plist "$contents/Info.plist"
ditto "$products/ClockWidget.appex" "$contents/PlugIns/ClockWidget.appex"
ditto "$products/ClockIntents.appex" "$contents/Extensions/ClockIntents.appex"

for extension in "$contents/PlugIns/ClockWidget.appex" "$contents/Extensions/ClockIntents.appex"; do
    test -d "$extension/Contents/Resources/Metadata.appintents"
    codesign --force --options runtime --timestamp=none --sign "$identity" \
        --entitlements macos/Extensions.entitlements "$extension"
done
codesign --force --options runtime --timestamp=none --sign "$identity" \
    --entitlements macos/Host.entitlements "$app"
codesign --verify --deep --strict "$app"
mv "$app" "$destination"
printf '%s\n' "$destination"
