#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked --release --features ui
package_name="httpied-linux-$(uname -m)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$package_name/docs" "$stage/$package_name/.github/workflows" dist
install -m 755 target/release/httpied "$stage/$package_name/httpied"
install -m 644 README.md "$stage/$package_name/README.md"
install -m 644 docs/WORKFLOW.md docs/IMPLEMENTATION.md docs/CHANGE_REPORT.md docs/UI_TESTING.md docs/FEATURE_PLAN.md "$stage/$package_name/docs/"
install -m 644 .github/workflows/check.yml "$stage/$package_name/.github/workflows/check.yml"
tar -czf "dist/$package_name.tar.gz" -C "$stage" "$package_name"
sha256sum "dist/$package_name.tar.gz" > "dist/$package_name.tar.gz.sha256"
