#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
version=${1:-0.1.0}
architecture=$(dpkg --print-architecture)
case "$architecture" in
    amd64|arm64) ;;
    *)
        echo "Debian packages are supported for amd64 and arm64; current architecture is $architecture." >&2
        exit 1
        ;;
esac

case "$version" in
    ''|*[!0-9A-Za-z.+:~-]*)
        echo "Invalid Debian package version: $version" >&2
        exit 1
        ;;
esac

output="$repo_root/dist/tuneka_${version}_${architecture}.deb"
stage="$repo_root/build/deb-root"
rm -rf "$stage"
mkdir -p "$(dirname -- "$output")"
mkdir -p "$stage/usr/lib/tuneka" \
    "$stage/usr/lib/tuneka/assets" \
    "$stage/usr/bin" \
    "$stage/usr/share/applications" \
    "$stage/usr/share/metainfo" \
    "$stage/usr/share/icons/hicolor/scalable/apps" \
    "$stage/usr/share/doc/tuneka"

install -m 0644 "$repo_root/app.py" "$repo_root/ui.py" "$repo_root/i18n.py" "$stage/usr/lib/tuneka/"
install -m 0644 "$repo_root/assets/tuneka.png" "$stage/usr/lib/tuneka/assets/"
install -m 0755 "$repo_root/packaging/debian/tuneka.in" "$stage/usr/bin/tuneka"
install -m 0644 "$repo_root/packaging/debian/io.github.Delored.Tuneka.desktop" "$stage/usr/share/applications/"
install -m 0644 "$repo_root/packaging/debian/io.github.Delored.Tuneka.metainfo.xml" "$stage/usr/share/metainfo/"
install -m 0644 "$repo_root/packaging/icons/io.github.Delored.Tuneka.svg" "$stage/usr/share/icons/hicolor/scalable/apps/"
install -m 0644 "$repo_root/LICENSE" "$stage/usr/share/doc/tuneka/LICENSE"
install -m 0644 "$repo_root/packaging/debian/copyright" "$stage/usr/share/doc/tuneka/copyright"

mkdir -p "$stage/DEBIAN"
sed -e "s/@VERSION@/$version/" -e "s/@ARCHITECTURE@/$architecture/" \
    "$repo_root/packaging/debian/control.in" > "$stage/DEBIAN/control"
dpkg-deb --build --root-owner-group "$stage" "$output"

package_architecture=$(dpkg-deb --field "$output" Architecture)
if [ "$package_architecture" != "$architecture" ]; then
    echo "Built package architecture $package_architecture does not match host architecture $architecture." >&2
    exit 1
fi
echo "Built $output"
