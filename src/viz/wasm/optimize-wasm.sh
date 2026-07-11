#!/usr/bin/env bash
# Optimize wasm module(s) in place. Writes to a unique temp file, validates it,
# then atomically renames into place — so a concurrent build, dev-server rebuild,
# or interrupted run can't leave a truncated / stale-tail blob (the failure that
# shipped a corrupt geoscript_analysis to prod). Invalid output fails the build.
set -euo pipefail

# WASM_OPT_LEVEL=O2 for dev builds; -O2 matches -O4's output size at ~3x the speed
flags=(-g "-${WASM_OPT_LEVEL:-O4}" --enable-simd --enable-relaxed-simd --enable-nontrapping-float-to-int
  --precompute-propagate --strip-dwarf -c --enable-bulk-memory)

for f in "$@"; do
  tmp="$(mktemp "${f}.XXXXXX")"
  trap 'rm -f "$tmp"' EXIT
  wasm-opt "$f" "${flags[@]}" -o "$tmp"
  wasm-validate "$tmp"
  mv -f "$tmp" "$f"
  trap - EXIT
done
