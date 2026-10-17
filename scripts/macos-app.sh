#!/usr/bin/env bash
# Assembles the macOS application from built desktop binaries: OxidGene.app,
# its binaries joined into one universal executable, signed ad hoc, in a disk
# image beside a link to /Applications. Runs on macOS, which provides lipo,
# sips, iconutil, codesign and hdiutil.
#
#   scripts/macos-app.sh <dmg> <binary>...
#
# Ad hoc signing seals the bundle but proves no identity: Gatekeeper still
# asks the user to allow the first opening (docs/quickstart.md §1). A
# Developer ID signature and Apple's notarization replace it once the project
# has an Apple developer account (docs/roadmap.md §8).
set -euo pipefail

if (($# < 2)); then
    echo "usage: $0 <dmg> <binary>..." >&2
    exit 2
fi
dmg=$1
shift

root=$(cd "$(dirname "$0")/.." && pwd)
# The bundle's identifier: WebKit keeps the window's data under it.
identifier=io.github.trois-six.oxidgene
# The oldest macOS the bundle accepts; the binaries are built for it
# (MACOSX_DEPLOYMENT_TARGET).
minimum_system=11.0
pkgid=$(cargo pkgid --manifest-path "$root/Cargo.toml" --package oxidgene-desktop)
version=${pkgid##*[#@]}
# A bundle version is numbers only: a pre-release keeps its base version.
bundle_version=${version%%-*}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
staging="$work/OxidGene"
app="$staging/OxidGene.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

lipo -create -output "$app/Contents/MacOS/oxidgene-desktop" "$@"

# The icon, at every size the 256-pixel source covers.
iconset="$work/OxidGene.iconset"
mkdir "$iconset"
icon_size() {
    sips -z "$1" "$1" "$root/assets/desktop/icon.png" --out "$iconset/$2" >/dev/null
}
for size in 16 32 128 256; do
    icon_size "$size" "icon_${size}x${size}.png"
    if ((size * 2 <= 256)); then
        icon_size $((size * 2)) "icon_${size}x${size}@2x.png"
    fi
done
iconutil --convert icns --output "$app/Contents/Resources/OxidGene.icns" "$iconset"

cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>OxidGene</string>
    <key>CFBundleExecutable</key>
    <string>oxidgene-desktop</string>
    <key>CFBundleIconFile</key>
    <string>OxidGene</string>
    <key>CFBundleIdentifier</key>
    <string>$identifier</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>OxidGene</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$bundle_version</string>
    <key>CFBundleVersion</key>
    <string>$bundle_version</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.reference</string>
    <key>LSMinimumSystemVersion</key>
    <string>$minimum_system</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - "$app"
codesign --verify --strict "$app"

ln -s /Applications "$staging/Applications"
mkdir -p "$(dirname "$dmg")"
# hdiutil fails now and then on CI runners with "Resource busy".
for attempt in 1 2 3; do
    if hdiutil create -volname OxidGene -srcfolder "$staging" -ov -format UDZO "$dmg"; then
        exit 0
    fi
    echo "hdiutil failed (attempt $attempt), retrying" >&2
    sleep 5
done
exit 1
