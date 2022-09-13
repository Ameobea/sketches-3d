import * as Comlink from 'comlink';

let engineP: Promise<WebAssembly.Instance> | null = null;

const methods = {
  setWasmBytes: async (bytes: Uint8Array) => {
    engineP = WebAssembly.compile(bytes).then(module => WebAssembly.instantiate(module, { env: {} }));
  },
  genNormalMap: async (packNormalGBA: boolean, imageData: Uint8Array, height: number, width: number) => {
    const engine = await engineP;
    if (!engine) {
      throw new Error('WASM not loaded');
    }

    const packMode = packNormalGBA ? 1 : 0;

    const texPtr: number = (engine.exports.malloc as Function)(imageData.length);
    let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    memory.set(imageData, texPtr);

    const generatedMapDataPtr: number = (engine.exports.gen_normal_map_from_texture as Function)(
      texPtr,
      height,
      width,
      packMode
    );
    // Update memory in case it was resized
    memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    const generatedMapData = new Uint8Array(
      memory.buffer.slice(generatedMapDataPtr, generatedMapDataPtr + height * width * 4)
    );
    (engine.exports.free as Function)(generatedMapDataPtr);
    (engine.exports.free as Function)(texPtr);

    return Comlink.transfer(generatedMapData, [generatedMapData.buffer]);
  },
};

Comlink.expose(methods);
