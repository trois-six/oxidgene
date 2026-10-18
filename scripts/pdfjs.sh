#!/usr/bin/env bash
# Vendors pdf.js (docs/development.md §2.6): downloads pdfjs-dist VERSION from
# the npm registry, checks the archive against INTEGRITY, and copies the files
# the PDF viewer loads into assets/public/pdfjs/, which the web bundle serves
# as they are and the desktop embeds.
#
#   scripts/pdfjs.sh VERSION INTEGRITY
#
# INTEGRITY is the archive's Subresource Integrity string, `sha512-…`, as the
# registry lists it (`npm view pdfjs-dist@VERSION dist.integrity`). The legacy
# build is the one taken: the modern build requires what only the latest
# Chrome and Firefox implement, and the desktop draws in the platform's
# WebView — WebKitGTK, WKWebView, WebView2.
set -Eeuo pipefail

if [[ $# -ne 2 || ! $2 =~ ^sha512- ]]; then
    echo "usage: $0 VERSION sha512-INTEGRITY" >&2
    exit 2
fi
version=$1
integrity=$2
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "$version is not a release version" >&2
    exit 2
fi

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination="$root/assets/public/pdfjs"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --silent --show-error --location --proto '=https' \
    "https://registry.npmjs.org/pdfjs-dist/-/pdfjs-dist-$version.tgz" -o "$work/pdfjs.tgz"
actual=$(python3 -I -c 'import base64, hashlib, sys
print("sha512-" + base64.b64encode(hashlib.sha512(open(sys.argv[1], "rb").read()).digest()).decode())' \
    "$work/pdfjs.tgz")
if [[ $actual != "$integrity" ]]; then
    echo "pdfjs-dist $version does not match its integrity: expected $integrity, got $actual" >&2
    exit 1
fi

mkdir "$work/package"
tar --extract --gzip --no-same-owner --no-same-permissions \
    --file "$work/pdfjs.tgz" --directory "$work/package" --strip-components 1 package
package="$work/package"

rm -rf "$destination"
mkdir -p "$destination/wasm" "$destination/images"
# The library, its worker, the viewer components and their stylesheet.
install -m 644 \
    "$package/legacy/build/pdf.min.mjs" \
    "$package/legacy/build/pdf.worker.min.mjs" \
    "$package/legacy/web/pdf_viewer.mjs" \
    "$package/legacy/web/pdf_viewer.css" \
    "$package/LICENSE" \
    "$destination/"
# The one picture of the stylesheet the viewer shows: a page being drawn.
install -m 644 "$package/legacy/web/images/loading-icon.gif" "$destination/images/"
# The decoders of JPEG 2000 and JBIG2 images, the formats of many scans,
# with their licences. Neither the scripting sandbox nor the colour
# management module: scripts never run, and colours fall back to the
# alternate space a PDF declares.
install -m 644 \
    "$package/wasm/openjpeg.wasm" \
    "$package/wasm/jbig2.wasm" \
    "$package/wasm/LICENSE_OPENJPEG" \
    "$package/wasm/LICENSE_PDFJS_OPENJPEG" \
    "$package/wasm/LICENSE_JBIG2" \
    "$package/wasm/LICENSE_PDFJS_JBIG2" \
    "$destination/wasm/"
printf 'pdfjs-dist %s %s\n' "$version" "$integrity" > "$destination/VERSION"

echo "pdf.js $version is in ${destination#"$root"/}"
