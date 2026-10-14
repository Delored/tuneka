#!/bin/sh
# Build tooling only. Requires already-built native Linux executables.
set -eu
bin_dir=$(cd "${1:?binary directory required}" && pwd)
mkdir -p "${2:?output directory required}"
output_dir=$(cd "$2" && pwd)
version=${3:-0.1.0}
case "$version" in ''|*[!0-9A-Za-z.+~-]*) echo 'Invalid package version' >&2; exit 1;; esac
architecture=$(dpkg --print-architecture)
case "${4:-headless}" in
    headless) package_name=tuneka-rust; binaries='tuneka tuneka-tui tuneka-mcp'; extra_depends=''; description='CLI, terminal UI and MCP server sharing a native Rust media pipeline.';;
    desktop) package_name=tuneka-rust-gui; binaries=tuneka-gui; extra_depends='libvulkan1, xdg-utils, fonts-dejavu-core, '; description='Native GPUI desktop frontend. Requires an X11/Wayland session and Vulkan driver.';;
    *) echo 'Mode must be headless or desktop' >&2; exit 1;;
esac
destination="$output_dir/${package_name}_${version}_${architecture}.deb"
test ! -e "$destination" || { echo "Output already exists: $destination" >&2; exit 1; }
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
package="$work/package"
doc="$package/usr/share/doc/$package_name"
mkdir -p "$package/DEBIAN" "$package/usr/bin" "$doc" "$work/debian"
set --
for binary in $binaries; do
    install -m 755 "$bin_dir/$binary" "$package/usr/bin/$binary"
    set -- "$@" "-e$package/usr/bin/$binary"
done
cat > "$work/debian/control" <<'EOF'
Source: tuneka-rust
Section: utils
Priority: optional
Maintainer: Tuneka contributors <noreply@localhost>

Package: tuneka-rust
Architecture: any
Description: Tuneka native media tools
EOF
depends=$(cd "$work" && dpkg-shlibdeps -O "$@" | sed -n 's/^shlibs:Depends=//p')
test -n "$depends"
cat > "$package/DEBIAN/control" <<EOF
Package: $package_name
Version: $version
Architecture: $architecture
Section: utils
Priority: optional
Maintainer: Tuneka contributors <noreply@localhost>
Depends: ca-certificates, $extra_depends$depends
Description: Tuneka native media tools (local development build)
 $description
 This local package is not a published release.
EOF
if [ "$package_name" = tuneka-rust-gui ]; then
    mkdir -p "$package/usr/share/applications" "$doc/xattr"
    cat > "$package/usr/share/applications/tuneka-rust.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Tuneka
Exec=tuneka-gui
Icon=audio-x-generic
Terminal=false
Categories=AudioVideo;Audio;Video;
StartupWMClass=Tuneka
EOF
    cp "$root/vendor/xattr/LICENSE-APACHE" "$root/vendor/xattr/LICENSE-MIT" "$root/vendor/xattr/TUNEKA-PATCH.md" "$doc/xattr/"
fi
cp "$root/README.md" "$root/THIRD_PARTY_NOTICES.md" "$doc/"
cp -R "$root/licenses" "$doc/licenses"
cp /usr/share/common-licenses/GPL-3 "$doc/copyright"
dpkg-deb --build --root-owner-group "$package" "$destination"
