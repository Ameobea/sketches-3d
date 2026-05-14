import type * as THREE from 'three';

import { installClickRaycaster } from 'src/viz/util/clickRaycaster';

export interface RaycastSelectOpts {
  canvas: HTMLCanvasElement;
  camera: THREE.Camera;
  getCandidates: () => THREE.Object3D[];
  onSelect: (id: string | null) => void;
  /** Suppresses selection when a drag (e.g. gizmo) is in progress. */
  isDraggingGizmo: () => boolean;
}

export const installRaycastSelect = (opts: RaycastSelectOpts): (() => void) =>
  installClickRaycaster({
    canvas: opts.canvas,
    camera: opts.camera,
    shouldIgnore: opts.isDraggingGizmo,
    onClick: ({ raycaster }) => {
      const hits = raycaster.intersectObjects(opts.getCandidates(), false);
      for (const hit of hits) {
        const id = hit.object.userData?.sourceNodeId as string | undefined;
        if (id) {
          opts.onSelect(id);
          return;
        }
      }
      opts.onSelect(null);
    },
  });
