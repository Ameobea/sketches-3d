#!/usr/bin/env bash
# Optimize wasm module(s) in place. Writes to a unique temp file, validates it,
# then atomically renames into place — so a concurrent build, dev-server rebuild,
# or interrupted run can't leave a truncated / stale-tail blob (the failure that
# shipped a corrupt geoscript_analysis to prod). Invalid output fails the build.
set -euo pipefail

# WASM_OPT_MODE=prod (default): the full -O4 pipeline; dev: -O2 minus constraint-analysis, ~1/4 the
# time. Measurements in docs/wasm-opt-tuning.md. --strip-dwarf must precede -O: with -g set, binaryen
# silently skips every DWARF-invalidating pass while DWARF sections are present (raw cargo blobs).
feats=(--enable-simd --enable-nontrapping-float-to-int --enable-bulk-memory)
case "${WASM_OPT_MODE:-prod}" in
  prod) flags=(-g --strip-dwarf -O4 "${feats[@]}" --precompute-propagate) ;;
  dev) flags=(-g --strip-dwarf -O2 "${feats[@]}" --skip-pass=constraint-analysis) ;;
  *) echo "optimize-wasm.sh: unknown WASM_OPT_MODE=$WASM_OPT_MODE" >&2; exit 1 ;;
esac

for f in "$@"; do
  tmp="$(mktemp "${f}.XXXXXX")"
  trap 'rm -f "$tmp"' EXIT
  wasm-opt "$f" "${flags[@]}" -o "$tmp"
  wasm-validate "$tmp"
  mv -f "$tmp" "$f"
  trap - EXIT
done
