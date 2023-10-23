import * as Comlink from 'comlink';

import * as geodesicsMod from '../../../../geodesics/geodesics.js';
import * as stone from '../../../wasmComp/stone';

export class RuneGenCtx {
  private engine!: typeof import('../../../wasmComp/stone');
  private geodesics: any;
  private initialized: Promise<void>;

  constructor() {
    this.initialized = this.init();
  }

  public async init() {
    const [engine, geodesics] = await Promise.all([
      Promise.resolve(stone).then(async engine => {
        await engine.default('/stone_bg.wasm');
        return engine;
      }),
      Promise.resolve(geodesicsMod)
        .then(mod => {
          (mod.Geodesics as any).locateFile = (path: string) => `/${path}`;
          return mod.Geodesics;
        })
        .then(mod => mod({ locateFile: (path: string) => `/${path}` })),
    ]);

    this.engine = engine;
    this.geodesics = geodesics;
  }

  public awaitInit() {
    return this.initialized;
  }

  private project2DCoordsWithGeodesics = (
    targetMeshIndices: Uint32Array | Uint16Array,
    targetMeshVertices: Float32Array,
    pointsToProject: Float32Array,
    indices: Uint32Array | Uint16Array,
    midpoint: [number, number]
  ): Float32Array => {
    const HEAPF32 = () => this.geodesics.HEAPF32 as Float32Array;
    const HEAPU32 = () => this.geodesics.HEAPU32 as Uint32Array;

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

    const vec_f32 = (vals: number[] | Float32Array) =>
      vec_generic(this.geodesics.vector$float$, HEAPF32, vals);

    const vec_uint32 = (vals: number[] | Uint32Array | Uint16Array) =>
      vec_generic(this.geodesics.vector$uint32_t$, HEAPU32, vals);

    const from_vec_f32 = (vec: any): Float32Array => {
      const length = vec.size();
      const ptr = vec.data();
      return HEAPF32().subarray(ptr / 4, ptr / 4 + length);
    };

    const computed = this.geodesics.computeGeodesics(
      vec_uint32(targetMeshIndices),
      vec_f32(targetMeshVertices),
      vec_f32(pointsToProject),
      vec_uint32(indices),
      midpoint[0],
      midpoint[1]
    );
    return from_vec_f32(computed.projectedPositions).slice();
  };

  public generate = ({
    indices: rawTargetMeshIndices,
    vertices: rawTargetMeshVertices,
  }: {
    indices: Uint32Array | Uint16Array;
    vertices: Float32Array;
  }) => {
    let targetMeshIndices: Uint32Array | Uint16Array;
    let targetMeshVertices: Float32Array;
    if (rawTargetMeshIndices instanceof Uint32Array) {
      const dedupCtxPtr = this.engine.dedup_vertices_u32(rawTargetMeshIndices, rawTargetMeshVertices);
      targetMeshIndices = this.engine.get_deduped_indices_u32(dedupCtxPtr);
      targetMeshVertices = this.engine.get_deduped_vertices_u32(dedupCtxPtr);
      this.engine.free_dedup_vertices_output_u32(dedupCtxPtr);
    } else if (rawTargetMeshIndices instanceof Uint16Array) {
      const dedupCtxPtr = this.engine.dedup_vertices_u16(rawTargetMeshIndices, rawTargetMeshVertices);
      targetMeshIndices = this.engine.get_deduped_indices_u16(dedupCtxPtr);
      targetMeshVertices = this.engine.get_deduped_vertices_u16(dedupCtxPtr);
      this.engine.free_dedup_vertices_output_u16(dedupCtxPtr);
    } else {
      throw new Error('Indices must be Uint32Array or Uint16Array');
    }

    const ctxPtr = this.engine.generate_rune_decoration_mesh_2d();
    const indices = this.engine.get_generated_indices_2d(ctxPtr);
    const vertices = this.engine.get_generated_vertices_2d(ctxPtr);
    this.engine.free_generated_runes_2d(ctxPtr);

    const projectedVertices = this.project2DCoordsWithGeodesics(
      targetMeshIndices,
      targetMeshVertices,
      vertices,
      indices,
      [0, 0]
    );

    const ctxPtr3D = this.engine.extrude_3d_mesh_along_normals(indices, projectedVertices);
    const extrudedIndices = this.engine.get_generated_indices_3d(ctxPtr3D);
    const extrudedVertices = this.engine.get_generated_vertices_3d(ctxPtr3D);
    const vertexNormals = this.engine.get_vertex_normals_3d(ctxPtr3D);
    this.engine.free_generated_runes_3d(ctxPtr3D);

    return Comlink.transfer({ indices: extrudedIndices, vertices: extrudedVertices, vertexNormals }, [
      extrudedIndices.buffer,
      extrudedVertices.buffer,
      vertexNormals.buffer,
    ]);
  };

  /**
   *
   * @returns Buffer in format [depth, minx, miny, maxx, maxy]
   */
  public debugAABB = () => {
    // return this.engine.debug_aabb_tree();
  };
}
