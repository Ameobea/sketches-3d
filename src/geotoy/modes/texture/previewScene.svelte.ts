// 3D preview of a mesh-tab object inside a texture tab. Private to texture mode: its objects
// live under one group in the shared viz scene, materials come from the shared runtime, and
// the camera helpers are the mesh mode's. Scene environment stays owned by `MeshScene`.

import * as THREE from 'three';

import type { MeshTabView, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { populateScene } from 'src/geoscript/runner/geoscriptRunner';
import type { RunResult } from 'src/geoscript/runner/runner';
import type { RenderedObject } from 'src/geoscript/runner/types';
import {
  applyCameraView,
  centerView,
  toggleProjection as toggleProjectionCamera,
} from 'src/geotoy/modes/mesh/cameraControls';
import { getView } from 'src/geotoy/modules/compositionStorage';
import type { MaterialRuntime } from 'src/geotoy/modules/materialRuntime.svelte';
import { removeRenderedObject, runtimeMaterialFor, schedulePomRescan } from 'src/geotoy/modules/sceneObjects';
import { DefaultView } from 'src/geotoy/types';
import type { Viz } from 'src/viz';

interface PreviewSceneDeps {
  viz: Viz;
  materialRuntime: MaterialRuntime;
  bootSignal: AbortSignal;
}

export class PreviewScene {
  private readonly deps: PreviewSceneDeps;
  private readonly group = new THREE.Group();
  private objects: RenderedObject[] = $state.raw([]);
  cameraProjection = $state<'perspective' | 'orthographic'>('perspective');
  /** Frame the next populate: a fresh target, or first show without a saved camera. */
  autoFrame = false;
  private restoresInFlight = 0;

  readonly meshCount: number = $derived(this.objects.filter(o => o instanceof THREE.Mesh).length);
  readonly materialNames: Set<string> = $derived(
    new Set(this.objects.flatMap(o => (o instanceof THREE.Mesh ? [o.userData.materialName as string] : [])))
  );

  constructor(deps: PreviewSceneDeps) {
    this.deps = deps;
    this.group.name = 'texture-preview';
    $effect(() => {
      const byName = deps.materialRuntime.byName;
      const inlinePass = deps.viz.postprocessingController?.inlineEmissivePass;
      for (const obj of this.objects) {
        if (!(obj instanceof THREE.Mesh)) continue;
        const mat = runtimeMaterialFor(byName, obj.userData.materialName);
        obj.material = mat;
        if (inlinePass) {
          if (mat.userData.inlineEmissiveBypass) inlinePass.addMesh(obj);
          else inlinePass.removeMesh(obj);
        }
      }
      schedulePomRescan(deps.viz);
    });
  }

  /** Detach a preview object, unregistering meshes from the inline-emissive pass first. */
  private removeObject(obj: RenderedObject) {
    if (obj instanceof THREE.Mesh) {
      this.deps.viz.postprocessingController?.inlineEmissivePass?.removeMesh(obj);
    }
    removeRenderedObject(this.group, obj);
  }

  /** `result.objects` must already be filtered to what the preview shows. */
  consume(result: RunResult, tree: TreeDef, moduleNameToNodeId: Record<string, string>) {
    const { viz } = this.deps;
    if (!this.group.parent) viz.scene.add(this.group);
    const prevObjects = this.objects;
    const prev = new Map<string, RenderedObject>();
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as string | undefined;
      if (typeof key === 'string') prev.set(key, obj);
    }
    const populated = populateScene(this.group, result, { tree, moduleNameToNodeId, prev });
    this.objects = populated.objects;
    for (const obj of prevObjects) {
      const key = obj.userData.reuseKey as string | undefined;
      if (typeof key === 'string' && populated.reusedKeys.has(key)) continue;
      this.removeObject(obj);
    }
    if (this.autoFrame) {
      this.autoFrame = false;
      void centerView(viz, this.objects);
    }
  }

  clear = () => {
    for (const obj of this.objects) this.removeObject(obj);
    this.objects = [];
    this.group.removeFromParent();
  };

  focus = () => void centerView(this.deps.viz, this.objects);

  /** Null while a restore is in flight or before orbit controls exist (see `MeshScene`). */
  buildViewState = (): MeshTabView | null =>
    this.restoresInFlight > 0 || !this.deps.viz.orbitControls ? null : getView(this.deps.viz);

  setView = async (view: MeshTabView | null) => {
    if (!view) this.autoFrame = true;
    this.restoresInFlight += 1;
    try {
      const v = view ?? DefaultView;
      if (await applyCameraView(this.deps.viz, v, this.deps.bootSignal)) this.cameraProjection = v.projection;
    } finally {
      this.restoresInFlight -= 1;
    }
  };

  toggleProjection = () => {
    this.cameraProjection = toggleProjectionCamera(this.deps.viz);
  };
}
