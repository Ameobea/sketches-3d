import * as THREE from 'three';
import type { EvalRequest } from './evalResult';

/** Debug material override for all rendered meshes (matches the `n` / `w` / `shift+w` keybinds). */
export type MaterialOverrideMode = 'wireframe' | 'wireframe-xray' | 'normal';

/** The app surface the scene file's headless render/eval harness polls and stages against.
 *  Dies in the phase-6 RenderHarness rework (await-sequenced against the modules directly). */
export interface GeotoyRenderHarnessCtx {
  getLastRunOutcome: () => { type: 'ok'; stats: RunStats } | { type: 'err'; err: string | null } | null;
  getAreAllMaterialsLoaded: () => boolean;
  /** Instant fit-all framing, ignoring selection — used by transient render auto-framing. */
  autoFrameForRender: () => void;
  /** Eval-mode (`geotoy eval`): serialize the run's outputs — values, exports, prints,
   *  meshes, paths — to the JSON envelope. Call after a successful run. */
  buildEvalResultJson: (req: EvalRequest) => Promise<string>;
  toggleWireframe: () => void;
  toggleWireframeXray: () => void;
  toggleNormalMat: () => void;
}

export interface RunStats {
  runtimeMs: number;
  renderedMeshCount: number;
  renderedPathCount: number;
  renderedLightCount: number;
  totalVtxCount: number;
  totalFaceCount: number;
}

export const DefaultCameraPos = new THREE.Vector3(10, 10, 10);
export const DefaultCameraTarget = new THREE.Vector3(0, 0, 0);
export const DefaultCameraFOV = 60;
export const DefaultCameraZoom = 1;

export const IntFormatter = new Intl.NumberFormat(undefined, {
  style: 'decimal',
  maximumFractionDigits: 0,
});
