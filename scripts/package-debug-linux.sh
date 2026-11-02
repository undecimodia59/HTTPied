#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked --release --features ui,debug-logging --bin httpied-debug
package_name="httpied-debug-linux-$(uname -m)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$package_name" dist
install -m 755 target/release/httpied-debug "$stage/$package_name/httpied-debug"
install -m 644 docs/DEBUGGING.md "$stage/$package_name/DEBUGGING.md"
install -m 644 docs/INVESTIGATION_DNS.md "$stage/$package_name/INVESTIGATION_DNS.md"
install -m 644 docs/INVESTIGATION_HEADERS.md "$stage/$package_name/INVESTIGATION_HEADERS.md"
tar -czf "dist/$package_name.tar.gz" -C "$stage" "$package_name"
sha256sum "dist/$package_name.tar.gz" > "dist/$package_name.tar.gz.sha256"
