let LastError = '';
let Geodesics: any = null;

export const initGeodesics = () => {
  if (Geodesics) {
    return Promise.resolve();
  }

  return import('src/geodesics/geodesics.js')
    .then(mod => {
      (mod.Geodesics as any).locateFile = (path: string) => `/${path}`;
      return mod.Geodesics;
    })
    .then(mod => mod({ locateFile: (path: string) => `/${path}` }))
    .then(mod => {
      Geodesics = mod;
    });
};

export const trace_geodesic_path = (
  meshVerts: Float32Array,
  meshIndices: Uint32Array,
  path: Float32Array,
  fullPath: boolean
): Float32Array => {
  if (!Geodesics) {
    LastError = 'Geodesics module not initialized';
    return new Float32Array(1);
  }

  const HEAPF32 = () => Geodesics.HEAPF32 as Float32Array;
  const HEAPU32 = () => Geodesics.HEAPU32 as Uint32Array;

  const vec_generic = (
    vecCtor: new () => any,
    mem: () => Float32Array | Uint32Array,
    vals: number[] | Float32Array | Uint32Array | Uint16Array
  ) => {
    const vec = new vecCtor();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    const buf = mem().subarray(ptr / 4, ptr / 4 + vals.length);
    buf.set(vals);
    return vec;
  };

  const vec_f32 = (vals: number[] | Float32Array) => vec_generic(Geodesics.vector$float$, HEAPF32, vals);

  const vec_uint32 = (vals: number[] | Uint32Array | Uint16Array) =>
    vec_generic(Geodesics.vector$uint32_t$, HEAPU32, vals);

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32().subarray(ptr / 4, ptr / 4 + length);
  };

  // The geodesics path walking impl is designed to walk over indices of a triangle mesh,
  // but we just want to walk a path of points that is connected in order.
  //
  // This creates fakes around that by creating a set of indices representing degenerate
  // triangles to build the path.
  const numPairs = path.length - 1;
  const indices = new Uint32Array(numPairs * 3);
  for (let pairIx = 0; pairIx < numPairs; pairIx += 1) {
    indices[pairIx * 3] = pairIx;
    indices[pairIx * 3 + 1] = pairIx + 1;
    indices[pairIx * 3 + 2] = pairIx + 1;
  }

  try {
    const computed = Geodesics.computeGeodesics(
      vec_uint32(meshIndices),
      vec_f32(meshVerts),
      vec_f32(path),
      vec_uint32(indices),
      fullPath
    );
    const out = from_vec_f32(computed.projectedPositions).slice();
    // computed.delete();
    return out;
  } catch (err) {
    LastError = err instanceof Error ? err.message : String(err);
    return new Float32Array(1);
  }
};

export const get_geodesic_error = (): string => LastError;
