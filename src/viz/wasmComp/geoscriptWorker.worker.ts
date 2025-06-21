import * as Comlink from 'comlink';

import { initManifoldWasm } from './manifold';
import type { Light } from 'src/viz/scenes/geoscriptPlayground/lights';

const getGeoscript = () =>
  import('src/viz/wasmComp/geoscript_repl').then(async engine => {
    await engine.default();
    return engine;
  });

let Geoscript: typeof import('src/viz/wasmComp/geoscript_repl');

const initGeoscript = async () => {
  if (Geoscript) {
    return Geoscript;
  }
  Geoscript = await getGeoscript();
  return Geoscript;
};

const filterNils = <T>(arr: (T | null | undefined)[]): T[] => arr.filter((x): x is T => x != null);

const methods = {
  init: async () => {
    const [_manifold, repl] = await Promise.all([initManifoldWasm(), initGeoscript()]);
    return repl.geoscript_repl_init();
  },
  reset: (ctxPtr: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    return Geoscript.geoscript_repl_reset(ctxPtr);
  },
  eval: (ctxPtr: number, code: string, includePrelude: boolean) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    Geoscript.geoscript_repl_eval(ctxPtr, code, includePrelude);
  },
  getErr: (ctxPtr: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    return Geoscript.geoscript_repl_get_err(ctxPtr);
  },
  getRenderedMeshCount: (ctxPtr: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    return Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
  },
  getRenderedMesh: (ctxPtr: number, meshIx: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }

    const transform = Geoscript.geoscript_repl_get_rendered_mesh_transform(ctxPtr, meshIx);
    const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, meshIx);
    const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, meshIx);
    const normals = Geoscript.geoscript_repl_get_rendered_mesh_normals(ctxPtr, meshIx);

    return Comlink.transfer(
      { verts, indices, normals, transform },
      filterNils([verts.buffer, indices.buffer, normals?.buffer])
    );
  },
  getRenderedPathCount: (ctxPtr: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    return Geoscript.geoscript_get_rendered_path_count(ctxPtr);
  },
  getRenderedPathVerts: (ctxPtr: number, pathIx: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    const verts = Geoscript.geoscript_get_rendered_path(ctxPtr, pathIx);
    return Comlink.transfer(verts, [verts.buffer]);
  },
  getRenderedLightCount: (ctxPtr: number) => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    return Geoscript.geoscript_get_rendered_light_count(ctxPtr);
  },
  getRenderedLight: (ctxPtr: number, lightIx: number): Light => {
    if (!Geoscript) {
      throw new Error('Geoscript not initialized');
    }
    const light = JSON.parse(Geoscript.geoscript_get_rendered_light(ctxPtr, lightIx));
    return Comlink.transfer(light, []);
  },
};

export type GeoscriptWorkerMethods = typeof methods;

Comlink.expose(methods);
