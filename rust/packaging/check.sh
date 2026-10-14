#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo clippy --locked --workspace --exclude tuneka-gui --all-targets -- -D warnings
cargo test --locked --workspace --exclude tuneka-gui
# GUI needs platform SDKs; execute separately on an actual desktop:
# cargo clippy -p tuneka-gui --all-targets -- -D warnings
# cargo test -p tuneka-gui
