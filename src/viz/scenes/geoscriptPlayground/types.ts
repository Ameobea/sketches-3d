import * as THREE from 'three';

export interface ReplCtx {
  centerView: () => void;
  toggleWireframe: () => void;
  toggleNormalMat: () => void;
  getLastRunOutcome: () => { type: 'ok'; stats: RunStats } | { type: 'err'; err: string | null } | null;
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

export const IntFormatter = new Intl.NumberFormat(undefined, {
  style: 'decimal',
  maximumFractionDigits: 0,
});
