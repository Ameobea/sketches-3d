import { get, type Readable, type Writable } from 'svelte/store';
import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module.js';
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { initSentry } from 'src/sentry';
import { buildDefaultSfxConfig, SfxManager } from './audio/SfxManager';
import { type AddPlayerRegionContactCB, getAmmoJS, initBulletPhysics } from './collision';
import * as Conf from './conf';
import { InlineConsole } from './helpers/inlineConsole';
import { initPlayerKinematicsDebugger } from './helpers/playerKinematicsDebugger/playerKinematicsDebugger.svelte.ts';
import { initEulerDebugger, initPosDebugger } from './helpers/posDebugger';
import { initTargetDebugger } from './helpers/targetDebugger';
import { Inventory } from './inventory/Inventory';
import {
  buildDefaultSceneConfig,
  type DashConfig,
  type PlayerMoveSpeed,
  type SceneConfig,
  type SceneConfigLocation,
  type SceneDef,
  ScenesByName,
  type CustomControlsEntry,
} from './scenes';
import { setDefaultDistanceAmpParams } from './shaders/customShader';
import { clamp, mergeDeep } from './util';

export interface FpPlayerStateGetters {
  getVerticalVelocity: () => number;
  getVerticalOffset: () => number;
  getIsJumping: () => boolean;
  getJumpAxis: () => [number, number, number];
  getExternalVelocity: () => [number, number, number];
  getIsDashing: () => boolean;
  getIsOnGround: () => boolean;
}

export interface FirstPersonCtx {
  addTriMesh: (mesh: THREE.Mesh) => void;
  teleportPlayer: (pos: THREE.Vector3, rot?: THREE.Vector3) => void;
  reset: () => void;
  addBox: (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion
  ) => void;
  addCone: (pos: THREE.Vector3, radius: number, height: number, quat?: THREE.Quaternion) => void;
  addCompound: (
    pos: [number, number, number],
    children: {
      type: 'box';
      pos: [number, number, number];
      halfExtents: [number, number, number];
      quat?: THREE.Quaternion;
    }[],
    quat?: THREE.Quaternion
  ) => void;
  removeRigidBody: (rigidBody: any) => void;
  addHeightmapTerrain: (
    heightmapData: Float32Array,
    minHeight: number,
    maxHeight: number,
    gridResolutionX: number,
    gridResolutionY: number,
    worldSpaceWidth: number,
    worldSpaceLength: number
  ) => void;
  optimize: () => void;
  setFlyMode: (isFlyMode: boolean) => void;
  setGravity: (gravity: number) => void;
  clearCollisionWorld: () => void;
  addPlayerRegionContactCb: AddPlayerRegionContactCB;
  playerStateGetters: FpPlayerStateGetters;
  setMoveSpeed: (moveSpeed: PlayerMoveSpeed) => void;
  setSpawnPos: (pos: THREE.Vector3, rot: THREE.Vector3) => void;
  registerOnRespawnCb: (cb: () => void) => void;
  unregisterOnRespawnCb: (cb: () => void) => void;
  easyModeMovement: Readable<boolean>;
  registerDashCb: (cb: (curTimeSecs: number) => void) => void;
  deregisterDashCb: (cb: (curTimeSecs: number) => void) => void;
  registerJumpCb: (cb: (curTimeSecs: number) => void) => void;
  deregisterJumpCb: (cb: (curTimeSecs: number) => void) => void;
}

interface SetupFirstPersonArgs {
  locations: Record<string, SceneConfigLocation>;
  camera: THREE.PerspectiveCamera;
  spawnPos: {
    pos: THREE.Vector3;
    rot: THREE.Vector3;
  };
  registerBeforeRenderCb: (cb: (curTimeSecs: number, tDiffSecs: number) => void) => void;
  playerConf: SceneConfig['player'];
  gravity: number | undefined;
  inlineConsole: InlineConsole | null | undefined;
  dashConfig: Partial<DashConfig> | undefined;
  oobYThreshold: number | undefined;
  sfxManager: SfxManager;
  vizConfig: Writable<Conf.VizConfig>;
  canvas: HTMLCanvasElement;
}

const setupFirstPerson = async ({
  locations,
  camera,
  spawnPos,
  registerBeforeRenderCb,
  playerConf,
  gravity = 40,
  inlineConsole,
  dashConfig,
  oobYThreshold = -55,
  sfxManager,
  vizConfig,
  canvas,
}: SetupFirstPersonArgs): Promise<FirstPersonCtx> => {
  const keyStates: Record<string, boolean> = {};

  const playerColliderHeight = playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight;
  const playerColliderRadius = playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius;

  const Ammo = await getAmmoJS();
  const {
    updateCollisionWorld,
    addTriMesh,
    teleportPlayer,
    reset,
    addBox,
    addCone,
    addCompound,
    addHeightmapTerrain,
    optimize,
    setGravity,
    setFlyMode,
    clearCollisionWorld,
    addPlayerRegionContactCb,
    playerStateGetters,
    removeRigidBody,
    setMoveSpeed,
    easyModeMovement,
    registerDashCb,
    registerJumpCb,
    deregisterDashCb,
    deregisterJumpCb,
  } = await initBulletPhysics({
    camera,
    keyStates,
    Ammo,
    spawnPos,
    gravity,
    jumpSpeed: playerConf?.jumpVelocity ?? 20,
    playerColliderRadius,
    playerColliderHeight,
    playerMoveSpeed: playerConf?.moveSpeed,
    playerStepHeight: playerConf?.stepHeight,
    externalVelocityAirDampingFactor: playerConf?.externalVelocityAirDampingFactor,
    externalVelocityGroundDampingFactor: playerConf?.externalVelocityGroundDampingFactor,
    dashConfig,
    sfxManager,
    vizConfig,
  });

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

  if (window.location?.href.includes('localhost')) {
    document.body.addEventListener('mousedown', evt => {
      if (evt.button === 3) {
        (window as any).back();
      }
    });
  }

  let mouseSensitivity = get(vizConfig).controls.mouseSensitivity;
  vizConfig.subscribe(vizConf => {
    mouseSensitivity = vizConf.controls.mouseSensitivity;
  });
  const cameraEulerScratch = new THREE.Euler();
  canvas.addEventListener('mousemove', evt => {
    if (document.pointerLockElement === canvas) {
      cameraEulerScratch.setFromQuaternion(camera.quaternion, 'YXZ');

      cameraEulerScratch.y -= evt.movementX * mouseSensitivity * 0.001;
      cameraEulerScratch.x -= evt.movementY * mouseSensitivity * 0.001;

      // Clamp the camera's rotation to the range of -PI/2 to PI/2
      // This is so the camera doesn't flip upside down
      cameraEulerScratch.x = clamp(cameraEulerScratch.x, -Math.PI / 2 + 0.001, Math.PI / 2 - 0.001);

      camera.quaternion.setFromEuler(cameraEulerScratch);
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
  (window as any).tpos = (x: number, y: number, z: number) => teleportPlayer(new THREE.Vector3(x, y, z));

  window.onbeforeunload = function () {
    if ((window as any).recordPos) {
      localStorage.backPos = (window as any).recordPos();
    }
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

  if (localStorage.goBackOnLoad) {
    (window as any).back();
    delete localStorage.goBackOnLoad;
  }

  const onRespawnCBs: (() => void)[] = [];
  const registerOnRespawnCb = (cb: () => void) => onRespawnCBs.push(cb);
  const unregisterOnRespawnCb = (cb: () => void) => {
    const idx = onRespawnCBs.indexOf(cb);
    if (idx !== -1) {
      onRespawnCBs.splice(idx, 1);
    }
  };

  function teleportPlayerIfOOB() {
    if (camera.position.y <= oobYThreshold) {
      teleportPlayer(spawnPos.pos, spawnPos.rot);
      onRespawnCBs.forEach(cb => cb());
    }
  }

  const setSpawnPos = (pos: THREE.Vector3, rot: THREE.Vector3) => {
    spawnPos = { pos, rot };
  };

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

  return {
    addTriMesh,
    teleportPlayer,
    reset,
    addBox,
    addCone,
    addCompound,
    addHeightmapTerrain,
    optimize,
    setFlyMode,
    setGravity,
    clearCollisionWorld,
    addPlayerRegionContactCb,
    playerStateGetters,
    removeRigidBody,
    setMoveSpeed,
    setSpawnPos,
    registerOnRespawnCb,
    unregisterOnRespawnCb,
    easyModeMovement,
    registerDashCb,
    registerJumpCb,
    deregisterDashCb,
    deregisterJumpCb,
  };
};

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
  (window as any).recordPos = (window as any).getView;
};

let isBlurred = false;

const initPauseHandlers = (
  paused: Writable<boolean>,
  clock: THREE.Clock,
  viewMode: NonNullable<SceneConfig['viewMode']>['type'],
  canvas: HTMLCanvasElement
) => {
  let didManuallyLockPointer = false;
  let clockStopTime = 0;

  const maybePauseViz = () => {
    if (!get(paused) && !isBlurred) {
      return;
    }

    clockStopTime = clock.getElapsedTime();
    clock.stop();
  };

  const maybeResumeViz = () => {
    if (get(paused) || isBlurred) {
      return;
    }

    clock.start();
    clock.elapsedTime = clockStopTime;

    if (viewMode === 'firstPerson' && !document.pointerLockElement && didManuallyLockPointer) {
      canvas.requestPointerLock({ unadjustedMovement: true });
    }
  };

  window.addEventListener('blur', () => {
    isBlurred = true;
    maybePauseViz();
  });
  window.addEventListener('focus', () => {
    isBlurred = false;
    maybeResumeViz();
  });

  window.addEventListener('keydown', event => {
    if (event.code === 'Escape') {
      paused.update(p => !p);
      if (get(paused)) {
        maybePauseViz();
      } else {
        maybeResumeViz();
      }
    }
  });
  document.addEventListener('pointerlockchange', _evt => {
    if (isBlurred) {
      return;
    }
    paused.set(!document.pointerLockElement);
  });

  document.addEventListener('mousedown', () => {
    if (viewMode === 'firstPerson' && !get(paused)) {
      didManuallyLockPointer = true;
      // `unadjustedMovement` is needed to bypass mouse acceleration and prevent bad inputs
      // that happen in some cases when using high polling rate mice or something like that
      canvas.requestPointerLock({ unadjustedMovement: true });
    }
  });

  paused.subscribe(paused => {
    if (paused) {
      maybePauseViz();
    } else {
      maybeResumeViz();
    }
  });
};

const initCustomKeyHandlers = (customControlsEntries: CustomControlsEntry[] | undefined) => {
  if (!customControlsEntries) {
    return;
  }

  const eventMap = new Map<string, () => void>();
  for (const { key, action } of customControlsEntries) {
    eventMap.set(key, action);
  }

  document.addEventListener('keydown', event => {
    const action = eventMap.get(event.key.toLowerCase());
    action?.();
  });
};

export const buildViz = (paused: Writable<boolean>, sceneDef: SceneDef) => {
  try {
    (screen.orientation as any).lock('landscape').catch(() => 0);
  } catch (_err) {
    // pass
  }

  const clock = new THREE.Clock();

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x020202);

  const camera = new THREE.PerspectiveCamera(
    Conf.DEFAULT_FOV,
    window.innerWidth / window.innerHeight,
    0.07,
    3_000
  );
  camera.matrixAutoUpdate = true;
  camera.rotation.order = 'YXZ';

  const renderer = new THREE.WebGLRenderer({
    antialias: false,
    powerPreference: 'high-performance',
    stencil: false,
  });

  // backwards compat
  if (sceneDef.legacyLights) {
    renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
    renderer.useLegacyLights = true;
    THREE.ColorManagement.enabled = false;
  } else {
    THREE.ColorManagement.enabled = true;
    renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
  }

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

  const stats = new Stats.default();
  stats.dom.style.position = 'absolute';
  stats.dom.style.top = '0px';

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
          // console.log('swapping back to close mat', mesh.name);
        }
        mesh.material = baseMat;
      } else {
        if (mesh.material !== replacementMat) {
          // console.log('swapping to far mat', mesh.name);
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

  let renderOverride: ((timeDiffSeconds: number) => void) | null = null;
  const setRenderOverride = (cb: ((timeDiffSeconds: number) => void) | null) => {
    renderOverride = cb;
  };

  let animateHandle: number = 0;
  function animate() {
    if (isBlurred || get(paused)) {
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
    if ((window as any).recordPos) {
      (window as any).lastPos = (window as any).recordPos();
    }
    renderer.dispose();
    beforeRenderCbs.length = 0;
    afterRenderCbs.length = 0;

    if (animateHandle) {
      cancelAnimationFrame(animateHandle);
    }

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

    console.clear();
  };

  const inventory = new Inventory();

  const collisionWorldLoadedCbs: ((fpCtx: FirstPersonCtx) => void)[] = [];

  const sfxManager = new SfxManager();

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
    clock,
    sfxManager,
  };
};

export const applyGraphicsSettings = (viz: VizState, graphics: Conf.GraphicsSettings) => {
  viz.camera.fov = graphics.fov;
  viz.camera.updateProjectionMatrix();
};

export const applyAudioSettings = (audio: Conf.AudioSettings) => {
  delete localStorage.globalVolume;
  const ctx = new AudioContext();
  const GlobalVolumeNode = (ctx as any).globalVolume as GainNode;
  GlobalVolumeNode.gain.value = 0;
  GlobalVolumeNode.gain.linearRampToValueAtTime(audio.globalVolume, ctx.currentTime + 0.1);
  // TODO: Music volume
};

type VizStateBase = ReturnType<typeof buildViz>;

export interface VizState extends VizStateBase {
  fpCtx?: FirstPersonCtx;
}

export const initViz = (
  container: HTMLElement,
  {
    paused,
    sceneName: providedSceneName = Conf.DefaultSceneName,
    vizCb,
  }: {
    paused: Writable<boolean>;
    sceneName?: string;
    vizCb: (viz: VizState, vizConfig: Writable<Conf.VizConfig>, sceneConf: SceneConfig) => void;
  }
) => {
  initSentry();

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }

  const viz: VizState = buildViz(paused, sceneDef);
  (window as any).viz = viz;

  container.appendChild(viz.renderer.domElement);
  container.appendChild(viz.stats.dom);

  const inlineConsole = window.location.href.includes('localhost') || true ? new InlineConsole() : null;

  const { sceneName, sceneLoader: getSceneLoader, gltfName: providedGLTFName, extension = 'gltf' } = sceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  let loader = new GLTFLoader().setPath('/');
  if (sceneDef.needsDraco) {
    const dracoLoader = new DRACOLoader().setDecoderPath(
      'https://www.gstatic.com/draco/versioned/decoders/1.5.6/'
    );
    loader = loader.setDRACOLoader(dracoLoader);
  }

  let fpCtx: FirstPersonCtx | undefined;
  let destroyed = false;
  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    if (destroyed) {
      return;
    }
    providedSceneName = providedSceneName.toLowerCase();

    const scene = sceneName
      ? gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group()
      : new THREE.Group();

    const [sceneLoader, vizConfig] = await Promise.all([getSceneLoader(), Conf.getVizConfig()]);
    applyGraphicsSettings(viz, get(vizConfig).graphics);
    applyAudioSettings(get(vizConfig).audio);
    setDefaultDistanceAmpParams(null);
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene, get(vizConfig))) ?? {}),
    };
    vizCb(viz, vizConfig, sceneConf);

    if (sceneConf.renderOverride) {
      viz.setRenderOverride(sceneConf.renderOverride);
    }

    const normalizedLocations: Record<string, SceneConfigLocation> = {};
    for (const [locName, { pos, rot }] of Object.entries(sceneConf.locations)) {
      normalizedLocations[locName] = {
        pos: pos instanceof THREE.Vector3 ? pos : new THREE.Vector3(pos[0], pos[1], pos[2]),
        rot: rot instanceof THREE.Vector3 ? rot : new THREE.Vector3(rot[0], rot[1], rot[2]),
      };
    }

    (window as any).locations = () => Object.keys(sceneConf.locations);
    (window as any).exportScene = () =>
      import('./helpers/gltfExport').then(({ exportScene }) => exportScene(viz.scene));

    if (sceneConf.enableInventory) {
      // TODO: set up inventory CBs
    }

    initPauseHandlers(paused, viz.clock, sceneConf.viewMode.type, viz.renderer.domElement);
    initCustomKeyHandlers(sceneConf.customControlsEntries);

    viz.sfxManager.setConfig(mergeDeep(buildDefaultSfxConfig(), sceneConf.sfx ?? {}));
    viz.sfxManager.setVizConfig(vizConfig);

    if (sceneConf.viewMode.type === 'firstPerson') {
      viz.registerAfterRenderCb((curTimeSeconds, tDiffSeconds) =>
        viz.sfxManager.tick(tDiffSeconds, curTimeSeconds)
      );
      const spawnPos = (window as any).lastPos
        ? (() => {
            const lastPos = JSON.parse((window as any).lastPos);
            return {
              pos: new THREE.Vector3(lastPos.pos[0], lastPos.pos[1], lastPos.pos[2]),
              rot: new THREE.Vector3(lastPos.rot[0], lastPos.rot[1], lastPos.rot[2]),
            };
          })()
        : normalizedLocations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      fpCtx = await setupFirstPerson({
        locations: normalizedLocations,
        camera: viz.camera,
        spawnPos,
        registerBeforeRenderCb: viz.registerBeforeRenderCb,
        playerConf: sceneConf.player,
        gravity: sceneConf.gravity,
        inlineConsole,
        dashConfig: sceneConf.player?.dashConfig,
        oobYThreshold: sceneConf.player?.oobYThreshold,
        sfxManager: viz.sfxManager,
        vizConfig,
        canvas: viz.renderer.domElement,
      });
      viz.fpCtx = fpCtx;
    } else if (sceneConf.viewMode.type === 'orbit') {
      await setupOrbitControls(
        viz.renderer.domElement,
        viz.camera,
        sceneConf.viewMode.pos,
        sceneConf.viewMode.target
      );
    }

    viz.scene.add(scene);

    let vOffset = 0;
    if (sceneConf.debugPos) {
      vOffset += 24;
      initPosDebugger(viz, container, 0);
    }
    if (sceneConf.debugCamera) {
      vOffset += 24;
      initEulerDebugger(viz, container, vOffset);
    }
    if (sceneConf.debugTarget) {
      vOffset += 24;
      initTargetDebugger(viz, container, vOffset);
    }
    if (sceneConf.debugPlayerKinematics) {
      vOffset += 24;
      initPlayerKinematicsDebugger(viz, container, vOffset);
    }

    const traverseCollidable = function (obj: THREE.Object3D, cb: (obj: THREE.Object3D) => void) {
      if (obj.name.includes('nocollide') || obj.name.endsWith('far') || obj.userData.nocollide) {
        return;
      }

      cb(obj);

      const children = obj.children;

      for (let i = 0, l = children.length; i < l; i++) {
        traverseCollidable(children[i], cb);
      }
    };

    if (fpCtx) {
      const traverseCb = (obj: THREE.Object3D) => {
        const children = obj.children;
        obj.children = [];
        if (obj instanceof THREE.Mesh) {
          fpCtx!.addTriMesh(obj);
        }
        obj.children = children;
      };
      traverseCollidable(scene, traverseCb);

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
    loader.load(`${gltfName}.${extension}`, gltfLoadedCB);
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
