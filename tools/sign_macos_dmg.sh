#!/usr/bin/env bash
# Ad-hoc sign the .app inside a macOS DMG and write the DMG back out.
#
# cargo-packager always passes --timestamp to codesign, which is not valid for
# ad-hoc signatures, so the signing has to happen after the DMG is built.
#
# Usage: bash tools/sign_macos_dmg.sh <path-to-dmg> "<App Name>"

set -euo pipefail

dmg="${1:?usage: sign_macos_dmg.sh <path-to-dmg> <app-name>}"
app_name="${2:?usage: sign_macos_dmg.sh <path-to-dmg> <app-name>}"

workdir="$(mktemp -d)"
mountpoint="$workdir/mount"
readwrite_dmg="$workdir/readwrite.dmg"
final_dmg="$workdir/final.dmg"

cleanup() {
    hdiutil detach "$mountpoint" -force >/dev/null 2>&1 || true
    rm -rf "$workdir"
}
trap cleanup EXIT

mkdir -p "$mountpoint"

echo "Converting $dmg to a writable image..."
hdiutil convert "$dmg" -format UDRW -o "$readwrite_dmg" -quiet

echo "Mounting the writable image..."
hdiutil attach "$readwrite_dmg" -mountpoint "$mountpoint" -nobrowse -quiet

app="$mountpoint/$app_name.app"
if [ ! -d "$app" ]; then
    echo "Could not find $app in the DMG" >&2
    exit 1
fi

echo "Ad-hoc signing $app..."
# Clear extended attributes that can make codesign refuse the bundle.
xattr -cr "$app"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

echo "Repacking the signed DMG..."
hdiutil detach "$mountpoint" -quiet
hdiutil convert "$readwrite_dmg" -format UDZO -o "$final_dmg" -quiet
mv -f "$final_dmg" "$dmg"

echo "Signed $dmg"
