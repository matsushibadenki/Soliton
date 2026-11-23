#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

bundle="target/aarch64-apple-darwin/release/bundle"
app="$bundle/macos/Soliton.app"
output_dir="${SOLITON_DMG_OUTPUT_DIR:-$bundle/dmg}"
notary_profile="${SOLITON_DMG_NOTARY_PROFILE:-}"
if [[ -n "$notary_profile" && -z "${SOLITON_DMG_SIGN_IDENTITY:-}" ]]; then
    echo "SOLITON_DMG_NOTARY_PROFILE requires SOLITON_DMG_SIGN_IDENTITY." >&2
    exit 1
fi
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
candidate_dir=""
cleanup() {
    if [[ -n "$candidate_dir" ]]; then
        printf 'Notarization candidate and available receipts retained in %s\n' "$candidate_dir" >&2
    fi
    if [[ "$mounted" == 1 ]]; then
        hdiutil detach "$work/mount" -quiet || return
    fi
    rm -rf "$work"
}
trap cleanup EXIT

name="Soliton_${version}_aarch64.dmg"
mkdir -p "$work/staging" "$work/mount" "$output_dir"
output_dir=$(cd "$output_dir" && pwd -P)
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
artifact_dir="$work"
if [[ -n "$notary_profile" ]]; then
    candidate_dir=$(mktemp -d "$output_dir/.notary-${version}.XXXXXX")
    mv "$work/$name" "$work/$name.runtime.json" "$candidate_dir/"
    artifact_dir="$candidate_dir"
    xcrun notarytool submit "$artifact_dir/$name" --keychain-profile "$notary_profile" \
        --output-format json > "$artifact_dir/submission.json"
    submission_id=$(plutil -extract id raw -o - "$artifact_dir/submission.json")
    printf 'Apple notarization submission %s\n' "$submission_id"
    xcrun notarytool wait "$submission_id" --keychain-profile "$notary_profile" \
        --timeout 30m --output-format json > "$artifact_dir/result.json"
    status=$(plutil -extract status raw -o - "$artifact_dir/result.json")
    if [[ "$status" != Accepted ]]; then
        printf 'Apple notarization status: %s\n' "$status" >&2
        exit 1
    fi
    xcrun stapler staple "$artifact_dir/$name"
    xcrun stapler validate "$artifact_dir/$name"
    spctl --assess --type open --context context:primary-signature "$artifact_dir/$name"
fi
hdiutil attach -quiet -readonly -nobrowse -mountpoint "$work/mount" "$artifact_dir/$name"
mounted=1
test "$(readlink "$work/mount/Applications")" = /Applications
codesign --verify --deep --strict "$work/mount/Soliton.app"
if [[ -n "$notary_profile" ]]; then
    spctl --assess --type execute "$work/mount/Soliton.app"
fi
hdiutil detach "$work/mount" -quiet
mounted=0
(cd "$artifact_dir" && shasum -a 256 "$name" > "$name.sha256")
mv -f "$artifact_dir/$name" "$artifact_dir/$name.sha256" "$artifact_dir/$name.runtime.json" "$output_dir/"
if [[ -n "$candidate_dir" ]]; then
    printf 'Apple notarization receipts retained in %s\n' "$candidate_dir"
    candidate_dir=""
fi
printf '%s\n' "$output_dir/$name" "$output_dir/$name.sha256" "$output_dir/$name.runtime.json"
