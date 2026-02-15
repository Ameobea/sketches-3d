import * as THREE from 'three';

export interface ReplCtx {
  centerView: () => void;
  toggleWireframe: () => void;
  toggleWireframeXray: () => void;
  toggleNormalMat: () => void;
  toggleLightHelpers: () => void;
  toggleAxesHelper: () => void;
  getLastRunOutcome: () => { type: 'ok'; stats: RunStats } | { type: 'err'; err: string | null } | null;
  getAreAllMaterialsLoaded: () => boolean;
  run: () => void;
  snapView: (axis: 'x' | 'y' | 'z') => void;
  orbit: (axis: 'vertical' | 'horizontal', angle: number) => void;
  toggleRecording: () => void;
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
