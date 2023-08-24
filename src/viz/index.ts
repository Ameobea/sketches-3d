/**
 * Originally adapted from:
 * https://github.com/mrdoob/three.js/blob/master/examples/games_fps.html
 *
 * With many changes and additions.
 */

import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { getAmmoJS, initBulletPhysics } from './collision';
import * as Conf from './conf';
import { InlineConsole } from './helpers/inlineConsole';
import { initPosDebugger } from './helpers/posDebugger';
import { initTargetDebugger } from './helpers/targetDebugger';
import { Inventory } from './inventory/Inventory';
import { buildDefaultSceneConfig, type SceneConfig, ScenesByName } from './scenes';

interface FirstPersonCtx {
  addTriMesh: (mesh: THREE.Mesh) => void;
  teleportPlayer: (pos: THREE.Vector3, rot?: THREE.Vector3) => void;
  addBox: (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion
  ) => void;
  optimize: () => void;
  setFlyMode: (isFlyMode: boolean) => void;
  setGravity: (gravity: number) => void;
  clearCollisionWorld: () => void;
}

const setupFirstPerson = async (
  locations: SceneConfig['locations'],
  camera: THREE.Camera,
  spawnPos: {
    pos: THREE.Vector3;
    rot: THREE.Vector3;
  },
  registerBeforeRenderCb: (cb: (curTimeSecs: number, tDiffSecs: number) => void) => void,
  playerConf: SceneConfig['player'],
  gravity: number | undefined,
  inlineConsole: InlineConsole | null | undefined,
  enableDash: boolean,
  oobYThreshold = -55
): Promise<FirstPersonCtx> => {
  let GRAVITY = gravity ?? 40;
  let JUMP_VELOCITY = playerConf?.jumpVelocity ?? 20;
  let ON_FLOOR_ACCELERATION_PER_SECOND = playerConf?.movementAccelPerSecond?.onGround ?? 40;

  const keyStates: Record<string, boolean> = {};

  const playerColliderHeight = playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight;
  const playerColliderRadius = playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius;

  const Ammo = await getAmmoJS();
  const {
    updateCollisionWorld,
    addTriMesh,
    teleportPlayer,
    addBox,
    optimize,
    setGravity,
    setFlyMode,
    clearCollisionWorld,
  } = await initBulletPhysics(
    camera,
    keyStates,
    Ammo,
    spawnPos,
    GRAVITY,
    JUMP_VELOCITY,
    playerColliderRadius,
    playerColliderHeight,
    ON_FLOOR_ACCELERATION_PER_SECOND,
    enableDash
  );

  document.addEventListener('keydown', event => {
    if (inlineConsole?.isOpen) {
      return;
    }

    keyStates[event.code] = true;
  });

  document.addEventListener('keyup', event => {
    if (inlineConsole?.isOpen) {
      return;
    }

    keyStates[event.code] = false;
  });

  document.addEventListener('mousedown', () => {
    document.body.requestPointerLock();
  });

  document.body.addEventListener('mousemove', event => {
    if (document.pointerLockElement === document.body) {
      camera.rotation.y -= event.movementX / 500;
      camera.rotation.x -= event.movementY / 500;

      // Clamp the camera's rotation to the range of -PI/2 to PI/2
      // This is so the camera doesn't flip upside down
      camera.rotation.x = Math.max(-Math.PI / 2, Math.min(Math.PI / 2, camera.rotation.x));
    }
  });

  (window as any).tp = (posName: string) => {
    const location = locations[posName];
    if (location) {
      teleportPlayer(location.pos, location.rot);
    } else {
      console.warn(`No location found for ${posName}`);
    }
  };

  window.onbeforeunload = function () {
    localStorage.backPos = (window as any).recordPos();
  };

  (window as any).back = () => {
    const backPos = localStorage.backPos;
    if (!backPos) {
      console.warn('No back position found');
      return;
    }

    const { pos, rot } = JSON.parse(backPos);
    teleportPlayer(new THREE.Vector3(pos[0], pos[1], pos[2]), new THREE.Vector3(rot[0], rot[1], rot[2]));
  };

  function teleportPlayerIfOOB() {
    if (camera.position.y <= oobYThreshold) {
      teleportPlayer(spawnPos.pos, spawnPos.rot);
    }
  }

  registerBeforeRenderCb((curTimeSecs, tDiffSecs) => {
    const newPlayerPos = updateCollisionWorld(curTimeSecs, tDiffSecs);
    newPlayerPos.y += 0.5 * playerColliderHeight;
    camera.position.copy(newPlayerPos);

    teleportPlayerIfOOB();
  });

  (window as any).getPos = () =>
    camera.position
      .clone()
      .sub(new THREE.Vector3(0, playerColliderHeight / 2, 0))
      .toArray();
  (window as any).getRot = () => camera.rotation.toArray();
  (window as any).recordPos = () =>
    JSON.stringify({
      pos: (window as any).getPos(),
      rot: camera.rotation.toArray().slice(0, 3),
    });
  (window as any).fly = () => setFlyMode();

  return { addTriMesh, teleportPlayer, addBox, optimize, setFlyMode, setGravity, clearCollisionWorld };
};

const disposeScene = (scene: THREE.Scene) =>
  scene.traverse(o => {
    if (o instanceof THREE.Mesh) {
      o.geometry?.dispose();
      if (Array.isArray(o.material)) {
        o.material.forEach(m => m.dispose());
      } else {
        o.material?.dispose();
      }
    }
  });

const setupOrbitControls = async (
  canvas: HTMLCanvasElement,
  camera: THREE.Camera,
  pos: THREE.Vector3,
  target: THREE.Vector3
) => {
  const { OrbitControls } = await import('three/examples/jsm/controls/OrbitControls.js');
  const controls = new OrbitControls(camera, canvas);
  controls.enableDamping = true;
  controls.dampingFactor = 0.1;
  camera.position.set(pos.x, pos.y, pos.z);
  controls.target.set(target.x, target.y, target.z);
  controls.update();

  (window as any).getView = () =>
    console.log({ pos: camera.position.toArray(), target: controls.target.toArray() });
};

export const buildViz = () => {
  try {
    screen.orientation.lock('landscape').catch(() => 0);
  } catch (err) {
    // pass
  }

  const clock = new THREE.Clock();

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x020202);

  const camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, 3_000);
  camera.matrixAutoUpdate = true;
  camera.rotation.order = 'YXZ';

  const renderer = new THREE.WebGLRenderer({
    antialias: false,
    powerPreference: 'high-performance',
    stencil: false,
  });
  (window as any).renderer = renderer;
  // const ext = renderer.getContext().getExtension('WEBGL_compressed_texture_s3tc');
  const gl = renderer.getContext();
  const fragDerivExt = gl.getExtension('OES_standard_derivatives');
  if (fragDerivExt) {
    renderer.getContext().hint(fragDerivExt.FRAGMENT_SHADER_DERIVATIVE_HINT_OES, gl.NICEST);
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;

  const stats = Stats.default();
  stats.domElement.style.position = 'absolute';
  stats.domElement.style.top = '0px';

  const resizeCbs: (() => void)[] = [];
  const beforeRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];
  const afterRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];

  function onWindowResize() {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();

    renderer.setSize(window.innerWidth, window.innerHeight);

    resizeCbs.forEach(cb => cb());
  }
  window.addEventListener('resize', onWindowResize);

  const registerResizeCb = (cb: () => void) => resizeCbs.push(cb);
  const registerBeforeRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) =>
    beforeRenderCbs.push(cb);
  const unregisterBeforeRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = beforeRenderCbs.indexOf(cb);
    if (idx !== -1) {
      beforeRenderCbs.splice(idx, 1);
    }
  };

  const registerAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) =>
    afterRenderCbs.push(cb);
  const unregisterAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = afterRenderCbs.indexOf(cb);
    if (idx !== -1) {
      afterRenderCbs.splice(idx, 1);
    }
  };

  const distanceSwapEntries: {
    mesh: THREE.Mesh;
    baseMat: THREE.Material;
    replacementMat: THREE.Material;
    distance: number;
  }[] = [];
  registerBeforeRenderCb(() => {
    for (const { mesh, baseMat, replacementMat, distance } of distanceSwapEntries) {
      const distanceToCamera = camera.position.distanceTo(mesh.position);
      if (distanceToCamera < distance) {
        if (mesh.material !== baseMat) {
          console.log('swapping back to close mat', mesh.name);
        }
        mesh.material = baseMat;
      } else {
        if (mesh.material !== replacementMat) {
          console.log('swapping to far mat', mesh.name);
        }
        mesh.material = replacementMat;
      }
    }
  });

  const registerDistanceMaterialSwap = (mesh: THREE.Mesh, replacementMat: THREE.Material, distance = 150) => {
    const baseMat = mesh.material;
    if (!baseMat || Array.isArray(baseMat)) {
      throw new Error('Mesh must have a single material');
    }
    distanceSwapEntries.push({ mesh, baseMat, replacementMat, distance });
  };

  let isBlurred = false;
  let clockStopTime = 0;
  window.addEventListener('blur', () => {
    isBlurred = true;
    clockStopTime = clock.getElapsedTime();
    clock.stop();
  });
  window.addEventListener('focus', () => {
    isBlurred = false;
    clock.start();
    clock.elapsedTime = clockStopTime;
  });

  let renderOverride: ((timeDiffSeconds: number) => void) | null = null;
  const setRenderOverride = (cb: ((timeDiffSeconds: number) => void) | null) => {
    renderOverride = cb;
  };

  let animateHandle: number = 0;
  function animate() {
    if (isBlurred) {
      animateHandle = requestAnimationFrame(animate);
      return;
    }

    const deltaTime = clock.getDelta();
    const curTimeSeconds = clock.getElapsedTime();

    beforeRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    if (renderOverride) {
      renderOverride(deltaTime);
    } else {
      renderer.render(scene, camera);
    }

    afterRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    stats.update();

    animateHandle = requestAnimationFrame(animate);
  }

  const onDestroy = () => {
    (window as any).lastPos = (window as any).recordPos();
    renderer.dispose();
    beforeRenderCbs.length = 0;
    afterRenderCbs.length = 0;
    if (animateHandle) {
      cancelAnimationFrame(animateHandle);
    }
    disposeScene(scene);
    console.clear();
  };

  const inventory = new Inventory();

  const collisionWorldLoadedCbs: ((fpCtx: FirstPersonCtx) => void)[] = [];

  return {
    camera,
    renderer,
    stats,
    scene,
    animate,
    registerResizeCb,
    registerBeforeRenderCb,
    unregisterBeforeRenderCb,
    registerDistanceMaterialSwap,
    registerAfterRenderCb,
    unregisterAfterRenderCb,
    onDestroy,
    setRenderOverride,
    inventory,
    collisionWorldLoadedCbs,
  };
};

export type VizState = ReturnType<typeof buildViz>;

export const initViz = (container: HTMLElement, providedSceneName: string = Conf.DefaultSceneName) => {
  const viz = buildViz();

  container.appendChild(viz.renderer.domElement);
  container.appendChild(viz.stats.domElement);

  const inlineConsole = window.location.href.includes('localhost') || true ? new InlineConsole() : null;

  const loader = new GLTFLoader().setPath('/');

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }
  const { sceneName, sceneLoader: getSceneLoader, gltfName: providedGLTFName } = sceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  let fpCtx: FirstPersonCtx | undefined;
  let destroyed = false;
  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    if (destroyed) {
      return;
    }
    providedSceneName = providedSceneName.toLowerCase();

    let scene = sceneName
      ? gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group()
      : new THREE.Group();

    const sceneLoader = await getSceneLoader();
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene)) ?? {}),
    };

    if (sceneConf.renderOverride) {
      viz.setRenderOverride(sceneConf.renderOverride);
    }

    (window as any).locations = () => Object.keys(sceneConf.locations);

    if (sceneConf.enableInventory) {
      // TODO: set up inventory CBs
    }

    if (sceneConf.viewMode.type === 'firstPerson') {
      const spawnPos = (window as any).lastPos
        ? (() => {
            const lastPos = JSON.parse((window as any).lastPos);
            return {
              pos: new THREE.Vector3(lastPos.pos[0], lastPos.pos[1], lastPos.pos[2]),
              rot: new THREE.Vector3(lastPos.rot[0], lastPos.rot[1], lastPos.rot[2]),
            };
          })()
        : sceneConf.locations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      fpCtx = await setupFirstPerson(
        sceneConf.locations,
        viz.camera,
        spawnPos,
        viz.registerBeforeRenderCb,
        sceneConf.player,
        sceneConf.gravity,
        inlineConsole,
        sceneConf.player?.enableDash ?? true,
        sceneConf.player?.oobYThreshold
      );
    } else if (sceneConf.viewMode.type === 'orbit') {
      await setupOrbitControls(
        viz.renderer.domElement,
        viz.camera,
        sceneConf.viewMode.pos,
        sceneConf.viewMode.target
      );
    }

    viz.scene.add(scene);

    if (sceneConf.debugPos) {
      initPosDebugger(viz, container, 0);
    }
    if (sceneConf.debugTarget) {
      initTargetDebugger(viz, container, +!!sceneConf.debugPos * 24);
    }

    if (fpCtx) {
      const traverseCb = (obj: THREE.Object3D<THREE.Event>) => {
        const children = obj.children;
        obj.children = [];
        if (obj instanceof THREE.Mesh && !obj.name.includes('nocollide') && !obj.name.endsWith('far')) {
          fpCtx!.addTriMesh(obj);
        }
        obj.children = children;
      };
      scene.traverse(traverseCb);

      for (const cb of viz.collisionWorldLoadedCbs) {
        cb(fpCtx);
      }

      fpCtx.optimize();
    }

    // TODO: Combine with above
    scene.traverse(child => {
      if (child instanceof THREE.Mesh && !child.name.includes('background')) {
        if (child.userData.noLight) {
          child.castShadow = false;
          child.receiveShadow = false;
        } else {
          child.castShadow = true;
          child.receiveShadow = true;
        }
        if (child.userData.noCastShadow) {
          child.castShadow = false;
        }
        if (child.userData.noReceiveShadow) {
          child.receiveShadow = false;
        }
      }
    });

    viz.animate();
  };

  if (gltfName) {
    loader.load(`${gltfName}.gltf`, gltfLoadedCB);
  } else {
    gltfLoadedCB({ scenes: [] });
  }

  return {
    destroy() {
      destroyed = true;
      viz.onDestroy();
      inlineConsole?.destroy();
      fpCtx?.clearCollisionWorld();
    },
  };
};
