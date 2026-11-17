#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

bundle="target/aarch64-apple-darwin/release/bundle"
app="$bundle/macos/Soliton.app"
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
codesign --verify --deep --strict "$app"
if [[ "${SOLITON_DMG_PUBLIC:-0}" == 1 ]]; then
    xcrun stapler validate "$app"
    spctl --assess --type execute "$app"
fi

work=$(mktemp -d)
mounted=0
cleanup() {
    if [[ "$mounted" == 1 ]]; then
        hdiutil detach "$work/mount" -quiet || return
    fi
    rm -rf "$work"
}
trap cleanup EXIT

name="Soliton_${version}_aarch64.dmg"
mkdir -p "$work/staging" "$work/mount" "$bundle/dmg"
ditto "$app" "$work/staging/Soliton.app"
ln -s /Applications "$work/staging/Applications"
hdiutil create -quiet -volname Soliton -srcfolder "$work/staging" -format UDZO "$work/$name"
hdiutil verify "$work/$name"
hdiutil attach -quiet -readonly -nobrowse -mountpoint "$work/mount" "$work/$name"
mounted=1
test "$(readlink "$work/mount/Applications")" = /Applications
codesign --verify --deep --strict "$work/mount/Soliton.app"
hdiutil detach "$work/mount" -quiet
mounted=0
(cd "$work" && shasum -a 256 "$name" > "$name.sha256")
mv -f "$work/$name" "$work/$name.sha256" "$bundle/dmg/"
printf '%s\n' "$bundle/dmg/$name" "$bundle/dmg/$name.sha256"
