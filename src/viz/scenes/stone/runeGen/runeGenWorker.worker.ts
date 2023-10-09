import * as Comlink from 'comlink';

export class RuneGenCtx {
  private engine!: typeof import('../../../wasmComp/stone');
  private initialized: Promise<void>;

  constructor() {
    this.initialized = this.init();
  }

  public async init() {
    this.engine = await import('../../../wasmComp/stone');
    await this.engine.default();
  }

  public awaitInit() {
    return this.initialized;
  }

  public generate = () => {
    const ctxPtr = this.engine.generate_rune_decoration_mesh();
    const indices = this.engine.get_generated_indices(ctxPtr);
    const vertices = this.engine.get_generated_vertices(ctxPtr);
    this.engine.free_generated_runes(ctxPtr);
    return { indices, vertices };
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
