import * as Comlink from 'comlink';

export class RuneGenCtx {
  private engine!: typeof import('../../../wasmComp/stone');
  private geodesics: any;
  private initialized: Promise<void>;

  constructor() {
    this.initialized = this.init();
  }

  public async init() {
    const [engine, geodesics] = await Promise.all([
      import('../../../wasmComp/stone').then(async engine => {
        await engine.default();
        return engine;
      }),
      import('../../../../geodesics/geodesics.js')
        .then(mod => {
          console.log('mod', mod);
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
    targetMeshIndices: Uint32Array,
    targetMeshVertices: Float32Array,
    pointsToProject: Float32Array,
    midpoint: [number, number]
  ): { normals: Float32Array; positions: Float32Array } => {
    const HEAPF32 = this.geodesics.HEAPF32 as Float32Array;
    const HEAPU32 = this.geodesics.HEAPU32 as Uint32Array;

    const vec_generic = (
      vecCtor: new () => any,
      mem: Float32Array | Uint32Array,
      vals: number[] | Float32Array | Uint32Array
    ) => {
      const vec = new vecCtor();
      vec.resize(vals.length, 0);
      const ptr = vec.data();
      const buf = mem.subarray(ptr / 4, ptr / 4 + vals.length);
      buf.set(vals);
      return vec;
    };

    const vec_f32 = (vals: number[] | Float32Array) =>
      vec_generic(this.geodesics.vector$float$, HEAPF32, vals);

    const vec_uint32 = (vals: number[] | Uint32Array) =>
      vec_generic(this.geodesics.vector$uint32_t$, HEAPU32, vals);

    const from_vec_f32 = (vec: any): Float32Array => {
      const length = vec.size();
      const ptr = vec.data();
      return HEAPF32.subarray(ptr / 4, ptr / 4 + length);
    };

    const computed = this.geodesics.computeGeodesics(
      vec_uint32(targetMeshIndices),
      vec_f32(targetMeshVertices),
      vec_f32(pointsToProject),
      midpoint[0],
      midpoint[1]
    );
    const normals = from_vec_f32(computed.projectedNormals);
    const positions = from_vec_f32(computed.projectedPositions);
    return { normals, positions };
  };

  public generate = () => {
    // const ctxPtr = this.engine.generate_rune_decoration_mesh();
    // const indices = this.engine.get_generated_indices(ctxPtr);
    // const vertices = this.engine.get_generated_vertices(ctxPtr);
    // this.engine.free_generated_runes(ctxPtr);
    // return { indices, vertices };

    const ctxPtr = this.engine.generate_rune_decoration_mesh_2d();
    const indices = this.engine.get_generated_indices_2d(ctxPtr);
    const vertices = this.engine.get_generated_vertices_2d(ctxPtr);
    // this.engine.free_generated_runes_2d(ctxPtr);

    const scale = 10_000;
    const targetMeshVertices = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1].map(x => x * scale));
    const targetMeshIndices = new Uint32Array([0, 1, 2, 1, 0, 3, 0, 2, 3, 1, 3, 2]);

    const { normals, positions } = this.project2DCoordsWithGeodesics(
      targetMeshIndices,
      targetMeshVertices,
      vertices,
      [0, 0]
    );

    return { indices, vertices: positions };
  };

  /**
   *
   * @returns Buffer in format [depth, minx, miny, maxx, maxy]
   */
  public debugAABB = () => {
    return this.engine.debug_aabb_tree();
  };
}

Comlink.expose(new RuneGenCtx());
