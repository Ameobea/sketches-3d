import * as THREE from 'three';

import type { BehaviorFn } from '../sceneRuntime/types';

type Phase = 'default' | 'ready' | 'armed' | 'disabled';

interface Params {
  /** Strip active, player not standing on it. */
  defaultMaterial?: string;
  /** Player standing on the strip, not boosting. */
  readyMaterial?: string;
  /** Player standing on the strip and boost is effective (armed + aux held, or coyote window). */
  armedMaterial?: string;
  /** Strip's `boostSurfaceConfig` is currently undefined (e.g. pulse-off interval). */
  disabledMaterial?: string;
}

const boostSurfaceVisual: BehaviorFn = (params, entity, runtime) => {
  const p = params as unknown as Params;
  const targetMesh = entity.object instanceof THREE.Mesh ? entity.object : null;
  const baseline = targetMesh ? targetMesh.material : null;

  const cache: Partial<Record<Phase, THREE.Material | undefined>> = {};
  const lookups: Record<Phase, string | undefined> = {
    default: p.defaultMaterial,
    ready: p.readyMaterial,
    armed: p.armedMaterial,
    disabled: p.disabledMaterial,
  };

  let playerHere = false;
  let lastPhase: Phase | null = null;

  const unsubscribe = entity.addPlayerContactListener({
    onEnter: () => {
      playerHere = true;
    },
    onLeave: () => {
      playerHere = false;
    },
  });

  const resolveMat = (phase: Phase): THREE.Material | undefined => {
    if (phase in cache) return cache[phase];
    const name = lookups[phase];
    if (!name) return (cache[phase] = undefined);
    const built = runtime.viz.levelLoadHandle?.builtMaterials;
    if (!built) return undefined; // not ready yet — re-resolve next tick
    return (cache[phase] = built.get(name));
  };

  return {
    tick() {
      let phase: Phase;
      if (entity.boostSurfaceConfig === undefined) {
        phase = 'disabled';
      } else if (playerHere) {
        phase = runtime.viz.fpCtx?.playerController.isBoostEffective() ? 'armed' : 'ready';
      } else {
        phase = 'default';
      }
      if (phase === lastPhase) return;
      if (!targetMesh) {
        lastPhase = phase;
        return;
      }
      const mat = resolveMat(phase);
      if (lookups[phase] && !mat) return;
      lastPhase = phase;
      targetMesh.material = mat ?? baseline ?? targetMesh.material;
    },
    onReset() {
      lastPhase = null;
      playerHere = false;
      if (targetMesh && baseline) targetMesh.material = baseline;
    },
    onDestroy() {
      unsubscribe();
    },
  };
};

export default boostSurfaceVisual;
