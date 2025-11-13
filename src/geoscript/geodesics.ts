import { AsyncOnce } from 'src/viz/util/AsyncOnce';

let LastError = '';

const GeodesicsModule = new AsyncOnce(() =>
  import('src/geodesics/geodesics.js')
    .then(mod => {
      (mod.Geodesics as any).locateFile = (path: string) => `/${path}`;
      return mod.Geodesics;
    })
    .then(mod => mod({ locateFile: (path: string) => `/${path}` }))
);

export const initGeodesics = () => GeodesicsModule.get();

export const get_geodesics_loaded = (): boolean => GeodesicsModule.isSome();

export const trace_geodesic_path = (
  meshVerts: Float32Array,
  meshIndices: Uint32Array,
  path: Float32Array,
  fullPath: boolean,
  startPosWorld: Float32Array,
  upDirectionWorld: Float32Array
): Float32Array => {
  const Geodesics = GeodesicsModule.getOptSync();
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
  //
  // Also, the geodesic walker expects the path to be provided as absolute coordinates, but we
  // receive it as a list of movements.
  const numPairs = path.length - 1;
  const indices = new Uint32Array(numPairs * 3 + 3);
  const absPath = new Float32Array(path.length + 2);

  // movement from origin to first point
  absPath[0] = 0;
  absPath[1] = 0;
  indices[0] = 0;
  indices[1] = 1;
  indices[2] = 1;

  for (let pairIx = 0; pairIx < numPairs; pairIx += 1) {
    const dy = path[pairIx * 2];
    const dx = path[pairIx * 2 + 1];
    const y = absPath[pairIx * 2] + dy;
    const x = absPath[pairIx * 2 + 1] + dx;
    absPath[2 + pairIx * 2] = y;
    absPath[2 + pairIx * 2 + 1] = x;

    indices[3 + pairIx * 3] = pairIx + 1;
    indices[3 + pairIx * 3 + 1] = pairIx + 2;
    indices[3 + pairIx * 3 + 2] = pairIx + 2;
  }

  const vec_meshIndices = vec_uint32(meshIndices);
  const vec_meshVerts = vec_f32(meshVerts);
  const vec_path = vec_f32(absPath);
  const vec_indices = vec_uint32(indices);
  const vec_startPosWorld = vec_f32(startPosWorld);
  const vec_upDirectionWorld = vec_f32(upDirectionWorld);

  let computed: any = null;
  try {
    computed = Geodesics.computeGeodesics(
      vec_meshIndices,
      vec_meshVerts,
      vec_path,
      vec_indices,
      fullPath,
      vec_startPosWorld,
      vec_upDirectionWorld
    );
    const out = from_vec_f32(computed.projectedPositions).slice();
    return out;
  } catch (err) {
    LastError = err instanceof Error ? err.message : String(err);
    return new Float32Array(1);
  } finally {
    vec_meshIndices.delete();
    vec_meshVerts.delete();
    vec_path.delete();
    vec_indices.delete();
    vec_startPosWorld.delete();
    vec_upDirectionWorld.delete();
    computed?.delete();
  }
};

export const get_geodesic_error = (): string => LastError;
