import {
  compileTree,
  compileTreeModules,
  buildInjectedValues,
  qualifyModuleName,
} from 'src/geoscript/treeCodegen';
import { ROOT_NODE_NAME, type TreeDef, type TreeKind } from 'src/geoscript/geotoyAPIClient';
import type {
  GizmoValuesByModule,
  RenderedControl,
  RenderedGizmo,
  TextureParamsEntry,
} from 'src/geoscript/runner/types';
import { injectInputs } from './inputInjection';
import type { CompositionTabDef } from './types';
import type { InputsJson } from './paramVariants';

/**
 * Composition-run modules are tab-qualified (`<tabId>:<node>`), so `module/handle`-qualified
 * input keys authored against bare node names are re-prefixed to keep resolving.
 */
const qualifyCompInputKeys = (
  inputs: InputsJson | undefined,
  tree: TreeDef,
  tabId: string
): InputsJson | undefined => {
  if (!inputs) return inputs;
  const names = new Set(Object.values(tree.nodes).map(n => n.name));
  let changed = false;
  const out: InputsJson = {};
  for (const [k, v] of Object.entries(inputs)) {
    const slash = k.indexOf('/');
    if (slash > 0 && names.has(k.slice(0, slash))) {
      out[`${tabId}:${k}`] = v;
      changed = true;
    } else {
      out[k] = v;
    }
  }
  return changed ? out : inputs;
};

const tabAmbientOf = (tab: Pick<CompositionTabDef, 'id' | 'kind' | 'preludeEjected' | 'tree'>) => ({
  tabId: tab.id,
  preludeKind: tab.preludeEjected ? ('' as const) : tab.kind,
  globalsSource: tab.tree.globalsSource,
});

export interface CompositionRunInputs {
  tabId: string;
  modules: Record<string, string>;
  code: string;
  rootModuleName: string;
  preludeKind: TreeKind | undefined;
  tabAmbients: { tabId: string; preludeKind: TreeKind | ''; globalsSource: string }[];
  textureParams: TextureParamsEntry[];
  gizmoValues: GizmoValuesByModule;
}

/**
 * Assemble the worker-run inputs for a composition tree + its inlined dep tabs, mirroring
 * Geotoy's multi-tab `buildRunInput`: tab-qualified modules, per-tab ambients (deps first,
 * entry tab last), injected control values, and prepended side-effect imports of `render`
 * dep tabs' roots so their `render_texture` calls fire unconditionally.
 */
export const buildCompositionRunInputs = (
  root: Omit<CompositionTabDef, 'render'>,
  depTabs: readonly CompositionTabDef[],
  inputs: InputsJson | undefined
): CompositionRunInputs => {
  const compiled = compileTree(root.tree, root.id);
  const modules = compiled.modules;
  const rootModuleName = qualifyModuleName(ROOT_NODE_NAME, root.id);
  // Bare-named inputs spread only across the entry tab's own modules — dep tabs keep their
  // persisted control values unless targeted with a qualified `<tab>:<node>/<handle>` key.
  const entryModuleNames = [...Object.keys(modules), rootModuleName];
  const gizmoValues = buildInjectedValues(root.tree, root.id);
  const textureParams: TextureParamsEntry[] = [];
  for (const [name, p] of Object.entries(root.textureParams ?? {})) {
    textureParams.push({ tabId: root.id, name, ...p });
  }
  for (const dep of depTabs) {
    Object.assign(modules, compileTreeModules(dep.tree, dep.id));
    Object.assign(gizmoValues, buildInjectedValues(dep.tree, dep.id));
    for (const [name, p] of Object.entries(dep.textureParams ?? {})) {
      textureParams.push({ tabId: dep.id, name, ...p });
    }
  }
  const code =
    depTabs
      .filter(d => d.render)
      .map(d => `import { } from "${qualifyModuleName(ROOT_NODE_NAME, d.id)}"\n`)
      .join('') + compiled.rootSource;
  injectInputs(
    gizmoValues,
    qualifyCompInputKeys(inputs, root.tree, root.id),
    [...Object.keys(modules), rootModuleName],
    entryModuleNames
  );
  return {
    tabId: root.id,
    modules,
    code,
    rootModuleName,
    preludeKind: root.preludeEjected ? undefined : root.kind,
    tabAmbients: [...depTabs.map(tabAmbientOf), tabAmbientOf(root)],
    textureParams,
    gizmoValues,
  };
};

/**
 * Filter a composition run's reported controls/gizmos to the entry tab and strip its
 * module-name prefix, so editor consumers (param panels, gizmo overlays, `module/handle`
 * input keys, `buildModuleNameToNodeId(tree)` lookups) keep seeing bare node names.
 * Dep-tab declarations are dropped — they aren't the placement's own params.
 */
export const normalizeCompositionRunOutputs = <T extends RenderedControl | RenderedGizmo>(
  entries: T[],
  tabId: string
): T[] => {
  const prefix = `${tabId}:`;
  const out: T[] = [];
  for (const e of entries) {
    if (e.sourceModule == null) out.push(e);
    else if (e.sourceModule.startsWith(prefix)) {
      out.push({ ...e, sourceModule: e.sourceModule.slice(prefix.length) });
    }
  }
  return out;
};
