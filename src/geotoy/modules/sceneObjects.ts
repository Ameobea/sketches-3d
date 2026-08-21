import * as THREE from 'three';

import { FallbackMat, HiddenMat } from 'src/geoscript/materials';
import type { RenderedObject } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';

/** Pending build → HiddenMat; unknown material name → FallbackMat (matches the runner). */
export const runtimeMaterialFor = (
  byName: Record<string, { material: THREE.Material | null } | undefined>,
  name: string
): THREE.Material => {
  const entry = byName[name];
  return entry ? (entry.material ?? HiddenMat) : FallbackMat;
};

/** Detach a run object (and a directional/spot light's target) and release its geometry.
 *  Materials belong to the material runtime and are never disposed here. */
export const removeRenderedObject = (parent: THREE.Object3D, obj: RenderedObject) => {
  parent.remove(obj);
  if (
    (obj instanceof THREE.DirectionalLight || obj instanceof THREE.SpotLight) &&
    obj.userData.geotoyTarget instanceof THREE.Object3D
  ) {
    parent.remove(obj.userData.geotoyTarget);
  }
  if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) obj.geometry.dispose();
};

let pomRescanQueued = false;
/** Material swaps invalidate the bounded-silhouette manager's per-mesh registry. */
export const schedulePomRescan = (viz: Viz) => {
  if (pomRescanQueued) return;
  pomRescanQueued = true;
  queueMicrotask(() => {
    pomRescanQueued = false;
    viz.postprocessingController?.rescanPomMeshes();
  });
};
