#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

bundle="target/aarch64-apple-darwin/release/bundle"
app="$bundle/macos/Soliton.app"
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
codesign --verify --deep --strict "$app"
if [[ "${SOLITON_DMG_PUBLIC:-0}" == 1 && -n "${SOLITON_DMG_SIGN_IDENTITY:-}" ]]; then
    echo "Staple the signed app before using SOLITON_DMG_PUBLIC." >&2
    exit 1
fi
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
if [[ -n "${SOLITON_DMG_SIGN_IDENTITY:-}" ]]; then
    python=".soliton-runtime/python-build/cpython-3.12.16-macos-aarch64-none/bin/python3"
    "$python" scripts/sign-desktop-runtimes.py "$work/staging/Soliton.app/Contents/Resources" "$SOLITON_DMG_SIGN_IDENTITY"
    codesign --force --sign "$SOLITON_DMG_SIGN_IDENTITY" --timestamp --options runtime \
        --entitlements src-tauri/Entitlements.plist "$work/staging/Soliton.app"
fi
npm run check:distribution -- --app "$work/staging/Soliton.app" \
    --smoke --output "$work/$name.runtime.json"
ln -s /Applications "$work/staging/Applications"
hdiutil create -quiet -volname Soliton -srcfolder "$work/staging" -format UDZO "$work/$name"
if [[ -n "${SOLITON_DMG_SIGN_IDENTITY:-}" ]]; then
    codesign --sign "$SOLITON_DMG_SIGN_IDENTITY" --timestamp "$work/$name"
    codesign --verify --strict "$work/$name"
fi
hdiutil verify "$work/$name"
hdiutil attach -quiet -readonly -nobrowse -mountpoint "$work/mount" "$work/$name"
mounted=1
test "$(readlink "$work/mount/Applications")" = /Applications
codesign --verify --deep --strict "$work/mount/Soliton.app"
hdiutil detach "$work/mount" -quiet
mounted=0
(cd "$work" && shasum -a 256 "$name" > "$name.sha256")
mv -f "$work/$name" "$work/$name.sha256" "$work/$name.runtime.json" "$bundle/dmg/"
printf '%s\n' "$bundle/dmg/$name" "$bundle/dmg/$name.sha256" "$bundle/dmg/$name.runtime.json"
