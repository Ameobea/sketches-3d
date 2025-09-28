import * as Comlink from 'comlink';

import { initUVUnwrap, unwrapUVs, type UVUnwrapRes } from 'src/viz/wasm/uv_unwrap/uvUnwrap';

export interface UVUnwrapParams {
  nCones: number;
  flattenToDisk: boolean;
  mapToSphere: boolean;
  enableUVIslandRotation: boolean;
}

const methods = {
  uvUnwrap: async (
    verts: Float32Array,
    indices: Uint32Array,
    params: UVUnwrapParams
  ): Promise<UVUnwrapRes> => {
    await initUVUnwrap();

    const res = unwrapUVs(
      verts,
      indices,
      params.nCones,
      params.flattenToDisk,
      params.mapToSphere,
      params.enableUVIslandRotation
    );

    if (res.type === 'error') {
      return res;
    }

    return Comlink.transfer(res, [res.out.uvs.buffer, res.out.verts.buffer, res.out.indices.buffer]);
  },
};

Comlink.expose(methods);

export type UVUnwrapWorker = typeof methods;
