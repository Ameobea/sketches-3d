import * as Comlink from 'comlink';

import { initManifoldWasm } from './manifold';
import type { Light } from 'src/viz/scenes/geoscriptPlayground/lights';
import * as Geoscript from 'src/viz/wasmComp/geoscript_repl';
import { initGeodesics } from './geodesics';
import { initCGAL } from 'src/viz/wasm/cgal/cgal';
import { initClipper2 } from 'src/viz/wasm/clipper2/clipper2';
import { textToPath } from './text_to_path';

const initGeoscript = async () => {
  await Geoscript.default();
  return Geoscript;
};

const filterNils = <T>(arr: (T | null | undefined)[]): T[] => arr.filter((x): x is T => x != null);

export interface GeoscriptAsyncDeps {
  geodesics?: boolean;
  cgal?: boolean;
  text_to_path?: boolean;
  clipper2?: boolean;
}

const initAsyncDeps = (
  deps: GeoscriptAsyncDeps,
  argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>>
) => {
  const promises: Promise<void>[] = [];
  if (deps.geodesics) {
    promises.push(initGeodesics());
  }
  if (deps.cgal) {
    const cgalInit = initCGAL();
    if (cgalInit instanceof Promise) {
      promises.push(cgalInit);
    }
  }
  if (deps.clipper2) {
    const clipperInit = initClipper2();
    if (clipperInit instanceof Promise) {
      promises.push(clipperInit);
    }
  }
  if (deps.text_to_path) {
    const args = argsByKey.text_to_path;
    if (!args) {
      throw new Error('text_to_path dependency requires arguments');
    }

    const [text, fontFamily, fontSize, fontWeight, fontStyle, letterSpacing, width, height] = args;
    const widthNum = +(width || 0);
    const heightNum = +(height || 0);

    const convertedFontWeight = fontWeight
      ? isNaN(Number(fontWeight))
        ? fontWeight
        : Number(fontWeight)
      : undefined;

    promises.push(
      textToPath(
        text,
        {
          width: width && widthNum !== 0 ? +width : null,
          height: height && heightNum !== 0 ? +height : null,
        },
        {
          fontFamily,
          fontSize: fontSize ? +fontSize : undefined,
          fontWeight: convertedFontWeight,
          fontStyle: (fontStyle || undefined) as 'normal' | 'italic' | 'oblique' | undefined,
          letterSpacing: letterSpacing ? +letterSpacing : undefined,
        }
      ).then(() => {})
    );
  }

  if (!promises.length) {
    return null;
  }

  return Promise.all(promises);
};

const methods = {
  init: async () => {
    const [_manifold, repl] = await Promise.all([initManifoldWasm(), initGeoscript()]);
    return repl.geoscript_repl_init();
  },
  reset: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_reset(ctxPtr);
  },
  initAsyncDeps: async (
    deps: GeoscriptAsyncDeps,
    argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>>
  ) => {
    await initAsyncDeps(deps, argsByKey);
  },
  eval: async (ctxPtr: number, code: string, includePrelude: boolean) => {
    Geoscript.geoscript_repl_parse_program(ctxPtr, code, includePrelude);
    if (Geoscript.geoscript_repl_has_err(ctxPtr)) {
      return;
    }

    const deps: GeoscriptAsyncDeps = JSON.parse(Geoscript.geoscript_repl_get_async_dependencies(ctxPtr));
    const depsPromise = initAsyncDeps(deps, {});
    if (depsPromise) {
      await depsPromise;
    }

    Geoscript.geoscript_repl_eval(ctxPtr);
  },
  getErr: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_err(ctxPtr);
  },
  getRenderedMeshCount: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
  },
  getRenderedMeshIndicesWithMaterial: (ctxPtr: number, materialId: string) => {
    return Geoscript.geoscript_repl_get_rendered_mesh_indices_with_material(ctxPtr, materialId);
  },
  getRenderedMesh: (ctxPtr: number, meshIx: number) => {
    const transform = Geoscript.geoscript_repl_get_rendered_mesh_transform(ctxPtr, meshIx);
    const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, meshIx);
    const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, meshIx);
    const normals = Geoscript.geoscript_repl_get_rendered_mesh_normals(ctxPtr, meshIx);
    const material = Geoscript.geoscript_repl_get_rendered_mesh_material(ctxPtr, meshIx);

    return Comlink.transfer(
      { verts, indices, normals, transform, material },
      filterNils([verts.buffer, indices.buffer, normals?.buffer])
    );
  },
  getRenderedPathCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_path_count(ctxPtr);
  },
  getRenderedPathVerts: (ctxPtr: number, pathIx: number) => {
    const verts = Geoscript.geoscript_get_rendered_path(ctxPtr, pathIx);
    return Comlink.transfer(verts, [verts.buffer]);
  },
  getRenderedLightCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_light_count(ctxPtr);
  },
  getRenderedLight: (ctxPtr: number, lightIx: number): Light => {
    const light = JSON.parse(Geoscript.geoscript_get_rendered_light(ctxPtr, lightIx));
    return Comlink.transfer(light, []);
  },
  setMaterials: (ctxPtr: number, defaultMaterialID: string | null, availableMaterials: string[]) => {
    Geoscript.geoscript_set_default_material(ctxPtr, defaultMaterialID ?? undefined);
    Geoscript.geoscript_set_materials(ctxPtr, availableMaterials);
  },
  getPrelude: () => Geoscript.geoscript_repl_get_prelude(),
};

export type GeoscriptWorkerMethods = typeof methods;

Comlink.expose(methods);
