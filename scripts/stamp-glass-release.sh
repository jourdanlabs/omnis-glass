#!/usr/bin/env bash
# Record git HEAD + release binary sha256 next to GLASS_RELEASE_TARGET (or repo target/release).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${GLASS_RELEASE_TARGET:-$ROOT/target/release}"
ALLOW_DIRTY=false
if [ "${1:-}" = "--allow-dirty" ]; then ALLOW_DIRTY=true; shift; fi
if [ "$#" -ne 0 ]; then echo "usage: $0 [--allow-dirty]" >&2; exit 2; fi
CHANGES="$(git -C "$ROOT" status --porcelain --untracked-files=normal)"
DIRTY=false
if [ -n "$CHANGES" ]; then
    if [ "$ALLOW_DIRTY" != true ]; then
        printf 'refusing to stamp dirty working tree:\n%s\n' "$CHANGES" >&2
        exit 1
    fi
    DIRTY=true
fi
mkdir -p "$OUT"
HEAD="$(git -C "$ROOT" rev-parse HEAD)"
STAMP="$HEAD"
if [ "$DIRTY" = true ]; then STAMP="$HEAD-dirty"; fi
echo "$STAMP" > "$OUT/GLASS_BUILD_HEAD"
MANIFEST="$OUT/GLASS_RELEASE_MANIFEST.json"
OMNIS="$OUT/omnis-glass"
PANE="$OUT/glass-pane"
sha() { shasum -a 256 "$1" | awk '{print $1}'; }
cat > "$MANIFEST" <<EOF
{
  "git_head": "$HEAD",
  "dirty": $DIRTY,
  "omnis_glass": $( [ -f "$OMNIS" ] && printf '"%s"' "$(sha "$OMNIS")" || printf 'null' ),
  "glass_pane": $( [ -f "$PANE" ] && printf '"%s"' "$(sha "$PANE")" || printf 'null' )
}
EOF
echo "stamped $OUT/GLASS_BUILD_HEAD= $STAMP"
