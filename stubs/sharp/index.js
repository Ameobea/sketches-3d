// Stubbed via `resolutions` in package.json.  Real sharp arrives only through
// manifold-3d -> @gltf-transform/functions -> ndarray-pixels, none of which this app imports
// (it uses manifold-3d's `manifold.js` wasm entry only).  It costs 38MB of prebuilt libvips and
// its install script fails on any machine with a system libvips.  If this ever throws, something
// started genuinely needing sharp -- drop the resolution rather than working around it.
throw new Error('sharp is stubbed out in this project; see stubs/sharp/index.js');
