#!/usr/bin/env bash
# Builds the pinned upstream Manifold revision into the gitignored local package that
# `package.json` links as `manifold-3d`.  With `--if-needed` it is a no-op when that
# package already matches the pin below and this script, so `just run` and `just build`
# can call it unconditionally.
set -euo pipefail
script=$(realpath "$0")
cd "$(dirname "$script")/.."

commit=7c86359b679f2bdd39bc7111fe9694a61c7c364b
version=3.5.1-git.7c86359b

out=src/viz/wasmComp/manifold
# Kept under src/viz/wasm/ since Vite's dev watcher ignores that tree.
build_root=src/viz/wasm/manifold-build
source_dir=$build_root/source
wasm_build=$build_root/wasm
stamp="$commit $(sha256sum "$script" | cut -d' ' -f1)"

if [[ ${1:-} == --if-needed && -f $out/build.stamp && $(cat "$out/build.stamp") == "$stamp" ]]; then
  exit 0
fi

source "$HOME/emsdk/emsdk_env.sh" >/dev/null 2>&1

mkdir -p "$build_root"
[[ -d $source_dir/.git ]] || git clone https://github.com/elalish/manifold.git "$source_dir"
if [[ -n $(git -C "$source_dir" status --porcelain) ]]; then
  echo "Manifold build checkout has changes; refusing to switch its revision." >&2
  exit 1
fi
git -C "$source_dir" rev-parse --verify --quiet "$commit^{commit}" >/dev/null ||
  git -C "$source_dir" fetch origin "$commit"
git -C "$source_dir" checkout --detach "$commit"

emcmake cmake -S "$source_dir" -B "$wasm_build" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release -DMANIFOLD_TEST=OFF -DMANIFOLD_PAR=OFF \
  -DMANIFOLD_JSBIND=ON -DMANIFOLD_CBIND=OFF -DMANIFOLD_PYBIND=OFF \
  -DCMAKE_CXX_FLAGS='-msimd128 -fwasm-exceptions' \
  -DCMAKE_EXE_LINKER_FLAGS='-fwasm-exceptions -profiling -sINITIAL_MEMORY=67108864 -sSTACK_SIZE=33554432'
# ninja tracks only manifold.js, so a missing wasm has to force the link step.
[[ -f $wasm_build/bindings/wasm/manifold.wasm ]] || rm -f "$wasm_build/bindings/wasm/manifold.js"
cmake --build "$wasm_build" --target manifoldjs -j"$(nproc)"

mkdir -p "$out"
wasm-opt "$wasm_build/bindings/wasm/manifold.wasm" -O4 -g --enable-exception-handling \
  --enable-bulk-memory --enable-simd --enable-nontrapping-float-to-int \
  -o "$out/manifold.wasm"
cp "$wasm_build"/bindings/wasm/manifold.js "$out"/
cp "$source_dir"/bindings/wasm/manifold-{global,encapsulated}-types.d.ts "$out"/
cp "$source_dir"/bindings/wasm/manifold-root.d.ts "$out"/manifold.d.ts
cp "$source_dir"/LICENSE "$out"/
cat > "$out/package.json" <<EOF
{
  "name": "manifold-3d",
  "version": "$version",
  "private": true,
  "type": "module",
  "main": "manifold.js",
  "types": "manifold.d.ts",
  "license": "Apache-2.0",
  "exports": {
    ".": { "types": "./manifold.d.ts", "default": "./manifold.js" },
    "./manifold.wasm": "./manifold.wasm",
    "./manifold.js": "./manifold.js",
    "./package.json": "./package.json"
  }
}
EOF
echo "$stamp" > "$out/build.stamp"
