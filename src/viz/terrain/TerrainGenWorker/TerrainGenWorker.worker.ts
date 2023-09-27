import * as Comlink from 'comlink';

let terrainGenEngineP: Promise<WebAssembly.Instance> | null = null;

export type TerrainGenVariantParams =
  | { Hill: { octaves: number; wavelengths: number[]; seed: number } }
  | { OpenSimplex: { coordinate_scales: number[]; weights: number[]; seed: number } };

export interface TerrainGenParams {
  variant: TerrainGenVariantParams;
  magnitude: number;
}

const getEngine = async () => {
  const engine = await terrainGenEngineP;
  if (!engine) {
    throw new Error('Wasm not loaded');
  }
  return engine;
};

const malloc = (engine: WebAssembly.Instance, size: number) => (engine.exports.malloc as Function)(size);

const free = (engine: WebAssembly.Instance, ptr: number) => (engine.exports.free as Function)(ptr);

const methods = {
  setTerrainGenWasmBytes: async (bytes: Uint8Array) => {
    terrainGenEngineP = WebAssembly.compile(bytes).then(module =>
      WebAssembly.instantiate(module, {
        env: {
          log_error: (ptr: number, len: number) =>
            void getEngine().then(engine => {
              let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
              const buf = memory.slice(ptr, ptr + len);
              const str = new TextDecoder().decode(buf);
              console.error(str);
            }),
          log_msg: (ptr: number, len: number) =>
            void getEngine().then(engine => {
              let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
              const buf = memory.slice(ptr, ptr + len);
              const str = new TextDecoder().decode(buf);
              console.log(str);
            }),
        },
      })
    );
  },
  createTerrainGenCtx: async () => {
    const engine = await getEngine();
    return (engine.exports.create_terrain_gen_ctx as Function)();
  },
  setTerrainGenParams: async (ctxPtr: number, params: TerrainGenParams) => {
    const engine = await getEngine();

    const serializedParams = JSON.stringify(params);
    const encoded = new TextEncoder().encode(serializedParams);
    const paramsPtr = malloc(engine, encoded.byteLength);
    let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    memory.set(encoded, paramsPtr);

    (engine.exports.set_params as Function)(ctxPtr, paramsPtr, encoded.byteLength);

    free(engine, paramsPtr);
  },
  genHeightmap: async (
    ctxPtr: number,
    resolution: [number, number],
    worldSpaceBounds: { mins: [number, number]; maxs: [number, number] }
  ) => {
    const engine = await getEngine();

    const heightmapPtr = (engine.exports.gen_heightmap as Function)(
      ctxPtr,
      resolution[0],
      resolution[1],
      worldSpaceBounds.mins[0],
      worldSpaceBounds.mins[1],
      worldSpaceBounds.maxs[0],
      worldSpaceBounds.maxs[1]
    );
    let memory = new Uint8Array((engine.exports.memory as WebAssembly.Memory).buffer);
    // this copies the memory from the wasm module into a new buffer
    const buf = memory.slice(
      heightmapPtr,
      heightmapPtr + resolution[0] * resolution[1] * Float32Array.BYTES_PER_ELEMENT
    );
    free(engine, heightmapPtr);
    const heightmap = new Float32Array(buf.buffer);
    return Comlink.transfer(heightmap, [heightmap.buffer]);
  },
};

export type TerrainGenWorker = typeof methods;

Comlink.expose(methods);
