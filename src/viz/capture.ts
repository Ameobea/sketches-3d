import * as THREE from 'three';

import type { Viz } from '.';
import { mulberry32 } from './util/util';

type Vec3 = THREE.Vector3 | [number, number, number];
/** `feet`: `pos` is a player feet position (as named locations are) rather than the camera position. */
export type CapturePose = { pos: Vec3; rot?: Vec3; feet?: boolean } | { pos: Vec3; target: Vec3 };

const params = typeof window === 'undefined' ? null : new URLSearchParams(window.location.search);
export const isCapture = params?.get('capture') === '1';
export const isRenderMode = isCapture || params?.get('render') === 'true';
export const captureQuality = params?.get('quality') ?? 'high';

let pendingLoads = 0;
if (isCapture) {
  Math.random = mulberry32(0x9e3779b9);
  const m = THREE.DefaultLoadingManager;
  const { itemStart, itemEnd } = m;
  m.itemStart = url => {
    pendingLoads++;
    itemStart.call(m, url);
  };
  m.itemEnd = url => {
    pendingLoads--;
    itemEnd.call(m, url);
  };
}

const arr = (v: Vec3): [number, number, number] => (Array.isArray(v) ? v : [v.x, v.y, v.z]);
const until = async (pred: () => boolean) => {
  while (!pred()) await new Promise(r => setTimeout(r, 50));
};

const waitForAssets = async (viz: Viz) => {
  const h = viz.levelLoadHandle;
  await Promise.all([
    viz.sceneConf.loadingComplete,
    h?.objects,
    h?.complete,
    h?.character,
    h?.emissiveBypassMeshes,
  ]);
  if (viz.viewMode.type === 'orbit') await until(() => !!viz.orbitControls);
  await until(() => pendingLoads === 0);
  try {
    await viz.renderer.compileAsync(viz.scene, viz.camera);
  } catch (err) {
    console.warn('[capture] compileAsync failed', err);
  }
  await until(() => pendingLoads === 0);
};

/** Installs `window.vizReady` and the `?capture=1` extensions of `window.vizCapture`; returns a fn to call
 *  with the scene-init promise (which itself awaits the physics startup barriers). */
export const initCapture = (viz: Viz) => {
  // Nothing sim-driven (physics, spawners, behaviors) may advance before `freeze`, or its state would
  // depend on load timing.
  viz.captureFreeze();
  const w = window as any;
  let settle!: (init: Promise<unknown>) => void;
  w.vizReady = new Promise<void>((res, rej) => {
    settle = init => init.then(() => waitForAssets(viz)).then(res, rej);
  });

  const freeze = ({ time = 10, frames = 8 } = {}) => {
    viz.captureFreeze();
    viz.clock.elapsed = time;
    viz.fpCtx?.setPhysicsTime(time);
    const info = viz.renderer.info;
    for (let i = 0; i < frames; i++) {
      if (i === frames - 1) {
        info.autoReset = false;
        info.reset();
      }
      viz.captureFrame(1 / 60);
    }
    info.autoReset = true;
    const cam = viz.camera as THREE.PerspectiveCamera;
    const { calls, triangles, points, lines } = info.render;
    return {
      camera: { pos: cam.position.toArray(), quat: cam.quaternion.toArray(), fov: cam.fov },
      info: { programs: info.programs?.length ?? 0, calls, triangles, points, lines, ...info.memory },
    };
  };

  const resume = () => {
    viz.clock.reset();
    viz.captureResume();
  };

  const pose = (p: string | CapturePose) => {
    const loc = typeof p === 'string' ? viz.sceneConf.locations[p] : p;
    if (!loc) throw new Error(`unknown location "${p}"; have ${Object.keys(viz.sceneConf.locations)}`);
    viz.controlState.cameraControlEnabled = false;
    viz.controlState.movementEnabled = false;
    // not `visible = false`: DepthPass and PlayerCharacter re-show it every frame
    viz.sceneConf.player?.mesh?.removeFromParent();
    const cam = viz.camera;
    cam.position.fromArray(arr(loc.pos));
    if ('target' in loc) {
      const target = new THREE.Vector3().fromArray(arr(loc.target));
      if (viz.orbitControls) {
        viz.orbitControls.target.copy(target);
        viz.orbitControls.update();
      } else cam.lookAt(target);
    } else {
      if ((typeof p === 'string' || ('feet' in loc && loc.feet)) && viz.fpCtx)
        cam.position.y += viz.fpCtx.playerColliderCenterToFeetOffset + 0.5 * viz.fpCtx.playerColliderHeight;
      const [x, y, z] = arr(loc.rot ?? [0, 0, 0]);
      cam.rotation.set(x, y, z, 'YXZ');
    }
    if (cam instanceof THREE.PerspectiveCamera) {
      cam.fov = viz.vizConfig.current.graphics.fov;
      cam.updateProjectionMatrix();
    }
    cam.updateMatrixWorld();
  };

  w.vizCapture = { ...w.vizCapture, freeze, resume, pose };
  return settle;
};
