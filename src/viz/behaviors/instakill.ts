import type { BehaviorFn } from '../sceneRuntime/types';

interface InstakillParams {
  /** If true, the entity's rigid body is removed so the player passes through (sensor-only).
   *  Default false: rigid body stays solid; the sensor also fires on overlap. */
  passThrough?: boolean;
}

const instakill: BehaviorFn = (params, entity, runtime) => {
  const p = params as unknown as InstakillParams;
  const fp = runtime.viz.fpCtx;
  if (!fp) throw new Error('instakill behavior: fpCtx not initialized');

  if (p.passThrough && entity.body) {
    fp.removeCollisionObject(entity.body);
  }

  const sensor = fp.addPlayerRegionContactCb({ type: 'mesh', mesh: entity.object }, () =>
    runtime.viz.respawnPlayer()
  );

  return {
    onDestroy() {
      runtime.viz.fpCtx?.removePlayerRegionContactCb(sensor);
    },
  };
};

export default instakill;
