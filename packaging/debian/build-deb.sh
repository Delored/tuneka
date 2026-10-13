#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
version=${1:-0.1.0}
architecture=$(dpkg --print-architecture)
if [ "$architecture" != amd64 ]; then
    echo "This test package is configured for amd64; current architecture is $architecture." >&2
    exit 1
fi

stage="$repo_root/build/deb-root"
output="$repo_root/dist/tuneka_${version}_amd64.deb"
rm -rf "$stage"
mkdir -p "$(dirname -- "$output")"
mkdir -p "$stage/usr/lib/tuneka" \
    "$stage/usr/bin" \
    "$stage/usr/share/applications" \
    "$stage/usr/share/metainfo" \
    "$stage/usr/share/icons/hicolor/scalable/apps" \
    "$stage/usr/share/doc/tuneka"

install -m 0644 "$repo_root/app.py" "$repo_root/ui.py" "$repo_root/i18n.py" "$stage/usr/lib/tuneka/"
install -m 0755 "$repo_root/packaging/debian/tuneka.in" "$stage/usr/bin/tuneka"
install -m 0644 "$repo_root/packaging/debian/io.github.Delored.Tuneka.desktop" "$stage/usr/share/applications/"
install -m 0644 "$repo_root/packaging/debian/io.github.Delored.Tuneka.metainfo.xml" "$stage/usr/share/metainfo/"
install -m 0644 "$repo_root/packaging/icons/io.github.Delored.Tuneka.svg" "$stage/usr/share/icons/hicolor/scalable/apps/"
install -m 0644 "$repo_root/LICENSE" "$stage/usr/share/doc/tuneka/LICENSE"
install -m 0644 "$repo_root/packaging/debian/copyright" "$stage/usr/share/doc/tuneka/copyright"

mkdir -p "$stage/DEBIAN"
sed "s/@VERSION@/$version/" "$repo_root/packaging/debian/control.in" > "$stage/DEBIAN/control"
dpkg-deb --build --root-owner-group "$stage" "$output"
echo "Built $output"
