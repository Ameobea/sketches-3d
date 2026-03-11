import * as Comlink from 'comlink';

let normalGenEngineP: Promise<WebAssembly.Instance> | null = null;
let textureCrossfadeEngineP: Promise<WebAssembly.Instance> | null = null;

const methods = {
  setNormalGenWasmBytes: async (bytes: Uint8Array<ArrayBuffer>) => {
    normalGenEngineP = WebAssembly.compile(bytes).then(module =>
      WebAssembly.instantiate(module, { env: {} })
    );
  },
  setTextureCrossfadeWasmBytes: async (bytes: Uint8Array<ArrayBuffer>) => {
    textureCrossfadeEngineP = WebAssembly.compile(bytes).then(module =>
      WebAssembly.instantiate(module, { env: {} })
    );
  },
  genNormalMap: async (packNormalGBA: boolean, imageData: Uint8Array, height: number, width: number) => {
    let engine = null;

    while (!engine) {
      engine = await normalGenEngineP;
      if (!engine) {
        await new Promise(r => setTimeout(r, 10));
      }
    }

    const packMode = packNormalGBA ? 1 : 0;

    const texPtr: number = (engine.exports.malloc as (size: number) => number)(imageData.length);
    let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    memory.set(imageData, texPtr);

    const generatedMapDataPtr: number = (
      engine.exports.gen_normal_map_from_texture as (
        texPtr: number,
        height: number,
        width: number,
        packMode: number
      ) => number
    )(texPtr, height, width, packMode);
    // Update memory in case it was resized
    memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    const generatedMapData = new Uint8Array(
      memory.buffer.slice(generatedMapDataPtr, generatedMapDataPtr + height * width * 4)
    );
    (engine.exports.free as (ptr: number) => void)(generatedMapDataPtr);
    (engine.exports.free as (ptr: number) => void)(texPtr);

    return Comlink.transfer(generatedMapData, [generatedMapData.buffer]);
  },
  genCrossfadedTexture: async (textures: Uint8Array[], textureSize: number, threshold: number) => {
    const engine = await textureCrossfadeEngineP;
    if (!engine) {
      throw new Error('Wasm not loaded');
    }

    textures.forEach((tex, i) => {
      const bufPtr: number = (engine.exports.wasm_malloc as (size: number) => number)(tex.length);
      const memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
      memory.set(tex, bufPtr);

      (engine.exports.set_texture as (bufPtr: number, index: number) => void)(bufPtr, i);
    });

    const outPtr: number = (engine.exports.generate as (textureSize: number, threshold: number) => number)(
      textureSize,
      threshold
    );
    const memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    const out = new Uint8Array(
      memory.buffer.slice(outPtr, outPtr + textures.length * textureSize * textureSize * textures.length * 4)
    );

    const outSizeBytes = textures.length * textureSize * textureSize * textures.length * 4;
    (engine.exports.wasm_free as (ptr: number, size: number) => void)(outPtr, outSizeBytes);
    (engine.exports.reset as (size: number) => void)(textureSize * textureSize * 4);

    return Comlink.transfer(out, [out.buffer]);
  },
};

Comlink.expose(methods);
