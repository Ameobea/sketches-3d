import type { BehaviorFn } from '../sceneRuntime/types';

interface PulseBoostSurfaceParams {
  activeSeconds: number;
  inactiveSeconds: number;
  /** Phase offset inside the cycle; 0 = begin at t=0 of the active interval. */
  phaseSeconds?: number;
  config: { targetSpeed: number; jumpRetention: number };
}

const pulseBoostSurface: BehaviorFn = (params, entity) => {
  const p = params as unknown as PulseBoostSurfaceParams;
  const period = p.activeSeconds + p.inactiveSeconds;
  const phase = p.phaseSeconds ?? 0;
  let lastActive: boolean | null = null;

  return {
    tick(elapsed, e) {
      const t = (((elapsed + phase) % period) + period) % period;
      const active = t < p.activeSeconds;
      if (active !== lastActive) {
        e.setBoostSurfaceConfig(active ? p.config : undefined);
        lastActive = active;
      }
    },
    onReset() {
      lastActive = null;
      entity.setBoostSurfaceConfig(undefined);
    },
  };
};

export default pulseBoostSurface;
