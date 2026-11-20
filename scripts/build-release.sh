#!/usr/bin/env bash
# Build, enumerate, and stamp the exact release binaries used by the tests.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac
mkdir -p "$TARGET_DIR/release"
TARGET_DIR="$(cd "$TARGET_DIR" && pwd -P)"
EXPECTED_RELEASE="$TARGET_DIR/release"
if [ -n "${GLASS_RELEASE_TARGET:-}" ]; then
    mkdir -p "$GLASS_RELEASE_TARGET"
    CONFIGURED_RELEASE="$(cd "$GLASS_RELEASE_TARGET" && pwd -P)"
    if [ "$CONFIGURED_RELEASE" != "$EXPECTED_RELEASE" ]; then
        echo "GLASS_RELEASE_TARGET must be the release directory under CARGO_TARGET_DIR" >&2
        exit 1
    fi
fi
export GLASS_RELEASE_TARGET="$EXPECTED_RELEASE"
export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo clean --release --workspace
cargo build --release -p obscura-cli --bins --features render
cargo build --release -p glass-pane --features render
cargo nextest list --release --features render
HEAD="$(git rev-parse HEAD)"
OMNIS_VERSION="$("$GLASS_RELEASE_TARGET/omnis-glass" --version)"
PANE_VERSION="$("$GLASS_RELEASE_TARGET/glass-pane" --version)"
if [[ "$OMNIS_VERSION" != *"($HEAD, clean)" || "$PANE_VERSION" != *"($HEAD, clean)" ]]; then
    rm -f "$GLASS_RELEASE_TARGET/GLASS_RELEASE_MANIFEST.json" "$GLASS_RELEASE_TARGET/GLASS_BUILD_HEAD"
    printf 'release version mismatch: expected %s, clean\nomnis-glass: %s\nglass-pane: %s\n' "$HEAD" "$OMNIS_VERSION" "$PANE_VERSION" >&2
    exit 1
fi
"$ROOT/scripts/stamp-glass-release.sh"
printf '%s\n%s\n' "$OMNIS_VERSION" "$PANE_VERSION"
shasum -a 256 "$GLASS_RELEASE_TARGET/omnis-glass" "$GLASS_RELEASE_TARGET/glass-pane"
