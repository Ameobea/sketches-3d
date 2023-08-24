I have a custom fork of ammo.js here: https://github.com/Ameobea/ammo.js/tree/updates

I have a `Justfile` with a build script, but here it is in case that gets lost or something:

```sh
# Might need to do this or similar to get emscripten working
source ~/emsdk/emsdk_env.sh

cmake -B builds -DCLOSURE=0   -DALLOW_MEMORY_GROWTH=1
cd builds && make -j16
echo "export {Ammo};" >> builds/ammo.wasm.js

# should probably run wasm-opt as well
wasm-opt -c -O4 builds/ammo.wasm.wasm -g -o builds/ammo.wasm.wasm
```

That will produce two output files in `builds`: `builds/ammo.wasm.js` and `builds/ammo.wasm.wasm`.

Both of those need to be copied into `src/ammojs/` in this project and they'll be imported directly by the physics code.
