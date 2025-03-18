import { type Readable } from 'svelte/store';
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
  type SceneConfig,
  type SceneDef,
  ScenesByName,
  type CustomControlsEntry,
  DefaultTopDownCameraFOV,
  DefaultTopDownCameraRotation,
  DefaultTopDownCameraOffset,
  DefaultOOBThreshold,
} from './scenes';
import { setDefaultDistanceAmpParams } from './shaders/customShader';
import { clamp, mergeDeep, mix, type PopupScreenFocus } from './util/util.ts';
import type { AmmoInterface, BtCollisionObject, BtVec3 } from 'src/ammojs/ammoTypes.ts';
import { rwritable, type TransparentWritable } from './util/TransparentWritable.ts';

const computeCameraPos = (
  newPlayerPos: THREE.Vector3,
  viewMode: Extract<NonNullable<SceneConfig['viewMode']>, { type: 'firstPerson' | 'top-down' }>,
  playerColliderHeight: number
) => {
  switch (viewMode.type) {
    case 'firstPerson':
      return newPlayerPos.add(new THREE.Vector3(0, 0.5 * playerColliderHeight, 0));
    case 'top-down':
      switch (viewMode.cameraFocusPoint?.type) {
        case undefined:
        case null:
        case 'player':
          return newPlayerPos.add(viewMode.cameraOffset ?? DefaultTopDownCameraOffset);
        case 'fixed':
          return viewMode.cameraFocusPoint.pos
            .clone()
            .add(viewMode.cameraOffset ?? DefaultTopDownCameraOffset);
        default:
          viewMode.cameraFocusPoint satisfies never;
          throw new Error('Unknown camera focus point type');
      }

    default:
      viewMode satisfies never;
      throw new Error('Unsupported view mode');
  }
};

export interface FpPlayerStateGetters {
  getVerticalVelocity: () => number;
  getVerticalOffset: () => number;
  getIsJumping: () => boolean;
  getJumpAxis: () => [number, number, number];
  getExternalVelocity: () => [number, number, number];
  getIsDashing: () => boolean;
  getIsOnGround: () => boolean;
  getPlayerPos: () => [number, number, number];
}

export interface FirstPersonCtx {
  addTriMesh: (mesh: THREE.Mesh, colliderType?: 'static' | 'kinematic') => void;
  teleportPlayer: (pos: THREE.Vector3, rot?: THREE.Vector3) => void;
  reset: () => void;
  addBox: (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion,
    colliderType?: 'static' | 'kinematic'
  ) => void;
  addCone: (
    pos: THREE.Vector3,
    radius: number,
    height: number,
    quat?: THREE.Quaternion,
    colliderType?: 'static' | 'kinematic'
  ) => void;
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
  removeCollisionObject: (collisionObj: BtCollisionObject) => void;
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
  easyModeMovement: Readable<boolean>;
  registerDashCb: (cb: (curTimeSecs: number) => void) => void;
  deregisterDashCb: (cb: (curTimeSecs: number) => void) => void;
  registerJumpCb: (cb: (curTimeSecs: number) => void) => void;
  deregisterJumpCb: (cb: (curTimeSecs: number) => void) => void;
  Ammo: AmmoInterface;
  btvec3: (x: number, y: number, z: number) => BtVec3;
}

interface SetupFirstPersonArgs {
  viz: Viz;
  initialSpawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 };
}

const setupFirstPerson = async ({ viz, initialSpawnPos }: SetupFirstPersonArgs): Promise<FirstPersonCtx> => {
  const playerColliderShape = viz.sceneConf.player?.playerColliderShape ?? 'capsule';

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
    removeCollisionObject,
    easyModeMovement,
    registerDashCb,
    registerJumpCb,
    deregisterDashCb,
    deregisterJumpCb,
    btvec3,
  } = await initBulletPhysics({
    viz,
    Ammo,
    gravity: viz.sceneConf.gravity ?? 40,
    jumpSpeed: viz.sceneConf.player?.jumpVelocity ?? 20,
    playerColliderShape,
    externalVelocityAirDampingFactor: viz.sceneConf.player?.externalVelocityAirDampingFactor,
    externalVelocityGroundDampingFactor: viz.sceneConf.player?.externalVelocityGroundDampingFactor,
    dashConfig: viz.sceneConf.player?.dashConfig,
    initialSpawnPos,
    simulationTickRate: viz.sceneConf.simulationTickRate,
  });

  if (window.location?.href.includes('localhost')) {
    document.body.addEventListener('mousedown', evt => {
      if (evt.button === 3) {
        (window as any).back();
      }
    });
  }

  const cameraEulerScratch = new THREE.Euler();
  viz.renderer.domElement.addEventListener('mousemove', evt => {
    if (
      document.pointerLockElement !== viz.renderer.domElement ||
      viz.sceneConf.viewMode!.type !== 'firstPerson' ||
      !viz.controlState.cameraControlEnabled
    ) {
      return;
    }

    cameraEulerScratch.setFromQuaternion(viz.camera.quaternion, 'YXZ');

    const mouseSensitivity = viz.vizConfig.current.controls.mouseSensitivity;
    cameraEulerScratch.y -= evt.movementX * mouseSensitivity * 0.001;
    cameraEulerScratch.x -= evt.movementY * mouseSensitivity * 0.001;

    // Clamp the camera's rotation to the range of -PI/2 to PI/2
    // This is so the camera doesn't flip upside down
    cameraEulerScratch.x = clamp(cameraEulerScratch.x, -Math.PI / 2 + 0.1, Math.PI / 2 - 0.001);

    viz.camera.quaternion.setFromEuler(cameraEulerScratch);
  });

  (window as any).tp = (posName: string) => {
    const location = viz.sceneConf.locations[posName];
    const pos = Array.isArray(location.pos)
      ? new THREE.Vector3(location.pos[0], location.pos[1], location.pos[2])
      : location.pos;
    const rot = Array.isArray(location.rot)
      ? new THREE.Vector3(location.rot[0], location.rot[1], location.rot[2])
      : location.rot;
    if (location) {
      teleportPlayer(pos, rot);
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

  const teleportPlayerIfOOB = () => {
    if (viz.camera.position.y <= (viz.sceneConf.player?.oobYThreshold ?? DefaultOOBThreshold)) {
      viz.respawnPlayer();
    }
  };

  viz.registerBeforeRenderCb(
    (curTimeSecs, tDiffSecs) => {
      const newPlayerPos = updateCollisionWorld(curTimeSecs, tDiffSecs);
      if (viz.sceneConf.player?.mesh) {
        viz.sceneConf.player.mesh.position.copy(newPlayerPos);
      }

      if (viz.controlState.cameraControlEnabled) {
        const playerColliderHeight =
          viz.sceneConf.player?.colliderSize?.height ?? Conf.DefaultPlayerColliderHeight;
        newPlayerPos.y += 0.5 * playerColliderHeight;
        const cameraPos = computeCameraPos(
          newPlayerPos,
          viz.sceneConf.viewMode! as any,
          playerColliderHeight
        );
        viz.camera.position.copy(cameraPos);
      }

      teleportPlayerIfOOB();
    },
    // Setting this priority ensures that the physics simulation always runs last, after all user-supplied
    // callbacks have been called.  This avoids issues where the visual positions of objects that are
    // animated by the user don't line up with the collision world positions.
    Infinity
  );

  (window as any).getPos = () =>
    viz.camera.position
      .clone()
      .sub(
        new THREE.Vector3(
          0,
          (viz.sceneConf.player?.colliderSize?.height ?? Conf.DefaultPlayerColliderHeight) / 2,
          0
        )
      )
      .toArray();
  (window as any).getRot = () => viz.camera.rotation.toArray();
  (window as any).recordPos = () =>
    JSON.stringify({
      pos: (window as any).getPos(),
      rot: viz.camera.rotation.toArray().slice(0, 3),
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
    removeCollisionObject,
    easyModeMovement,
    registerDashCb,
    registerJumpCb,
    deregisterDashCb,
    deregisterJumpCb,
    Ammo,
    btvec3,
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

export const applyGraphicsSettings = (viz: Viz, graphics: Conf.GraphicsSettings) => {
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

export interface ControlState {
  cameraControlEnabled: boolean;
  movementEnabled: boolean;
}

interface ViewModeInterpolationState {
  startTimeSecs: number;
  durationSecs: number;
  startCameraPos: THREE.Vector3;
  startCameraRot: THREE.Euler;
  startCameraFov: number;
  endCameraPos: THREE.Vector3;
  endCameraRot: THREE.Euler;
  endCameraFov: number;
}

export class Viz {
  public camera: THREE.PerspectiveCamera;
  public renderer: THREE.WebGLRenderer;
  public stats: Stats;
  public clock: THREE.Clock = new THREE.Clock();
  public inventory: Inventory = new Inventory();
  public sfxManager: SfxManager = new SfxManager();
  public scene: THREE.Scene = new THREE.Scene();
  public paused: TransparentWritable<boolean>;
  public popupCalled: TransparentWritable<PopupScreenFocus>;
  public collisionWorldLoadedCbs: ((fpCtx: FirstPersonCtx) => void)[] = [];
  public fpCtx: FirstPersonCtx | undefined;
  /**
   * Persistent user-configurable settings, mostly set via the pause menu.
   */
  public vizConfig: TransparentWritable<Conf.VizConfig> = rwritable(Conf.loadVizConfig());
  public sceneConf!: SceneConfig;
  public keyStates: Record<string, boolean> = {};
  /**
   * Initially defined by the scene config, but can be changed at runtime.
   */
  public spawnPos!: { pos: THREE.Vector3; rot: THREE.Vector3 };
  public controlState: ControlState = { cameraControlEnabled: true, movementEnabled: true };

  private resizeCbs: (() => void)[] = [];
  private isDestroyed = false;
  private beforeRenderCbs: {
    cb: (curTimeSeconds: number, tDiffSeconds: number) => void;
    priority: number;
  }[] = [];
  private afterRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];
  private animateHandle: number = 0;
  private distanceSwapEntries: {
    mesh: THREE.Mesh;
    baseMat: THREE.Material;
    replacementMat: THREE.Material;
    distance: number;
  }[] = [];
  private renderOverride: ((timeDiffSeconds: number) => void) | null = null;
  private isBlurred = false;
  /**
   * State used to manage smoothly interpolating camera when switching view modes.
   */
  private viewModeInterpolationState: ViewModeInterpolationState | null = null;
  private onRespawnCBs: (() => void)[] = [];
  private inlineConsole = window.location.href.includes('localhost') ? new InlineConsole() : undefined;

  constructor(
    paused: TransparentWritable<boolean>,
    popupCalled: TransparentWritable<PopupScreenFocus>,
    sceneDef: SceneDef
  ) {
    this.paused = paused;
    this.popupCalled = popupCalled;

    try {
      (screen.orientation as any).lock('landscape').catch(() => 0);
    } catch (_err) {
      // pass
    }

    this.scene.background = new THREE.Color(0x020202);

    const near = 0.07;
    const far = 3_000;
    this.camera = new THREE.PerspectiveCamera(
      Conf.DEFAULT_FOV,
      window.innerWidth / window.innerHeight,
      near,
      far
    );
    this.camera.matrixAutoUpdate = true;
    this.camera.rotation.order = 'YXZ';

    this.renderer = new THREE.WebGLRenderer({
      // we do manual antialiasing when needed
      antialias: false,
      powerPreference: 'high-performance',
      stencil: false,
    });

    this.stats = new Stats.default();
    this.stats.dom.style.position = 'absolute';
    this.stats.dom.style.top = '0px';

    // backwards compat
    if (sceneDef.legacyLights) {
      this.renderer.useLegacyLights = true;
      THREE.ColorManagement.enabled = false;
    } else {
      THREE.ColorManagement.enabled = true;
    }
    this.renderer.outputColorSpace = THREE.LinearSRGBColorSpace;

    (window as any).renderer = this.renderer;
    // const ext = renderer.getContext().getExtension('WEBGL_compressed_texture_s3tc');
    const gl = this.renderer.getContext();
    const fragDerivExt = gl.getExtension('OES_standard_derivatives');
    if (fragDerivExt) {
      this.renderer.getContext().hint(fragDerivExt.FRAGMENT_SHADER_DERIVATIVE_HINT_OES, gl.NICEST);
    }
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;

    const stats = new Stats.default();
    stats.dom.style.position = 'absolute';
    stats.dom.style.top = '0px';

    document.addEventListener('keydown', event => {
      if (this.inlineConsole?.isOpen) {
        return;
      }

      this.keyStates[event.code] = true;
    });

    document.addEventListener('keyup', event => {
      if (this.inlineConsole?.isOpen) {
        return;
      }

      this.keyStates[event.code] = false;
    });

    window.addEventListener('resize', this.onWindowResize);

    this.registerBeforeRenderCb(() => {
      for (const { mesh, baseMat, replacementMat, distance } of this.distanceSwapEntries) {
        const distanceToCamera = this.camera.position.distanceTo(mesh.position);
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
  }

  public animate = () => {
    if (this.isBlurred || this.paused.current) {
      this.animateHandle = requestAnimationFrame(this.animate);
      return;
    }

    const deltaTime = this.clock.getDelta();
    const curTimeSeconds = this.clock.getElapsedTime();

    this.beforeRenderCbs.forEach(({ cb }) => cb(curTimeSeconds, deltaTime));

    if (this.renderOverride) {
      this.renderOverride(deltaTime);
    } else {
      this.renderer.render(this.scene, this.camera);
    }

    this.afterRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    this.stats.update();

    this.animateHandle = requestAnimationFrame(this.animate);
  };

  private onWindowResize = () => {
    this.camera.aspect = window.innerWidth / window.innerHeight;
    this.camera.updateProjectionMatrix();

    this.renderer.setSize(window.innerWidth, window.innerHeight);

    this.resizeCbs.forEach(cb => cb());
  };

  public callPopup = (screen: PopupScreenFocus) => {
    this.paused.update(p => !p);
    this.popupCalled.set(screen);
  };

  public setRenderOverride = (cb: ((timeDiffSeconds: number) => void) | null) => {
    this.renderOverride = cb;
  };

  public registerResizeCb = (cb: () => void) => this.resizeCbs.push(cb);

  public registerDistanceMaterialSwap = (
    mesh: THREE.Mesh,
    replacementMat: THREE.Material,
    distance = 150
  ) => {
    const baseMat = mesh.material;
    if (!baseMat || Array.isArray(baseMat)) {
      throw new Error('Mesh must have a single material');
    }
    this.distanceSwapEntries.push({ mesh, baseMat, replacementMat, distance });
  };

  public setViewMode = (
    newViewMode: NonNullable<SceneConfig['viewMode']>,
    transitionTimeSeconds = 0
  ): Promise<void> => {
    if (this.sceneConf.viewMode!.type === 'orbit' || newViewMode.type === 'orbit') {
      throw new Error('Switching to/from orbit mode dynamically is not supported');
    }

    let onComplete: () => void;
    const completePromise = new Promise<void>(resolve => {
      onComplete = resolve;
    });

    if (this.viewModeInterpolationState) {
      console.error('Already interpolating view mode');
      return Promise.resolve();
    }

    const endFOV =
      newViewMode.type === 'top-down'
        ? (newViewMode.cameraFOV ?? DefaultTopDownCameraFOV)
        : this.vizConfig.current.graphics.fov;
    const playerPos = this.fpCtx!.playerStateGetters.getPlayerPos();
    const endCameraPos = computeCameraPos(
      new THREE.Vector3(playerPos[0], playerPos[1], playerPos[2]),
      newViewMode,
      this.sceneConf.player?.colliderSize?.height ?? Conf.DefaultPlayerColliderHeight
    );
    const endCameraRot =
      newViewMode.type === 'top-down'
        ? (newViewMode.cameraRotation ?? DefaultTopDownCameraRotation).clone()
        : (() => {
            switch (this.sceneConf.viewMode!.type) {
              case 'top-down':
                return new THREE.Euler(0, Math.PI, 0, 'YXZ');
              default:
                const spawnRot = this.spawnPos.rot;
                return new THREE.Euler(0, 0, 0, 'YXZ').setFromVector3(spawnRot);
            }
          })();

    if (transitionTimeSeconds === 0) {
      this.camera.fov = endFOV;
      this.camera.updateProjectionMatrix();
      return Promise.resolve();
    }

    // movement + camera controls are disabled during the transition period
    this.controlState.cameraControlEnabled = false;
    this.controlState.movementEnabled = false;

    this.viewModeInterpolationState = {
      durationSecs: transitionTimeSeconds,
      startTimeSecs: this.clock.getElapsedTime(),
      startCameraPos: this.camera.position.clone(),
      startCameraRot: this.camera.rotation.clone(),
      startCameraFov: this.camera.fov,
      endCameraPos,
      endCameraRot,
      endCameraFov: endFOV,
    };

    const cb = (curTimeSeconds: number) => {
      const elapsed = curTimeSeconds - this.viewModeInterpolationState!.startTimeSecs;
      const t = clamp(elapsed / this.viewModeInterpolationState!.durationSecs, 0, 1);

      const cameraPos = new THREE.Vector3(
        mix(
          this.viewModeInterpolationState!.startCameraPos.x,
          this.viewModeInterpolationState!.endCameraPos.x,
          t
        ),
        mix(
          this.viewModeInterpolationState!.startCameraPos.y,
          this.viewModeInterpolationState!.endCameraPos.y,
          t
        ),
        mix(
          this.viewModeInterpolationState!.startCameraPos.z,
          this.viewModeInterpolationState!.endCameraPos.z,
          t
        )
      );
      // TODO: lerp this properly taking into account angle wrapping
      const cameraRot = new THREE.Euler(
        mix(
          this.viewModeInterpolationState!.startCameraRot.x,
          this.viewModeInterpolationState!.endCameraRot.x,
          t
        ),
        mix(
          this.viewModeInterpolationState!.startCameraRot.y,
          this.viewModeInterpolationState!.endCameraRot.y,
          t
        ),
        mix(
          this.viewModeInterpolationState!.startCameraRot.z,
          this.viewModeInterpolationState!.endCameraRot.z,
          t
        ),
        'YXZ'
      );
      this.camera.position.copy(cameraPos);
      this.camera.rotation.copy(cameraRot);
      this.camera.fov = mix(
        this.viewModeInterpolationState!.startCameraFov,
        this.viewModeInterpolationState!.endCameraFov,
        t
      );
      this.camera.updateProjectionMatrix();

      if (t >= 1) {
        this.controlState.cameraControlEnabled = true;
        this.controlState.movementEnabled = true;
        this.viewModeInterpolationState = null;
        this.sceneConf.viewMode = newViewMode;
        onComplete();
        this.unregisterBeforeRenderCb(cb);
      }
    };
    this.registerBeforeRenderCb(cb);

    return completePromise;
  };

  public setSpawnPos = (pos: THREE.Vector3, rot: THREE.Vector3) => {
    this.spawnPos = { pos, rot };
  };

  public initPauseHandlers = (viewMode: NonNullable<SceneConfig['viewMode']>['type']) => {
    let didManuallyLockPointer = false;
    let clockStopTime = 0;

    const maybePauseViz = () => {
      if (!this.paused.current && !this.isBlurred) {
        return;
      }

      clockStopTime = this.clock.getElapsedTime();
      this.clock.stop();
    };

    const maybeResumeViz = async () => {
      if (this.paused.current || this.isBlurred) {
        return;
      }

      this.clock.start();
      this.clock.elapsedTime = clockStopTime;

      if (
        (viewMode === 'firstPerson' || viewMode === 'top-down') &&
        !document.pointerLockElement &&
        didManuallyLockPointer
      ) {
        const canvas = this.renderer.domElement;
        try {
          await canvas.requestPointerLock({ unadjustedMovement: true });
        } catch (err) {
          if (err instanceof Error && err.name === 'NotSupportedError') {
            // some browsers/operating systems do not support the `unadjustedMovement` option
            await canvas.requestPointerLock();
          } else {
            console.error('Failed to get pointer lock: ', err);
          }
        }
      }
    };

    window.addEventListener('blur', () => {
      this.isBlurred = true;
      maybePauseViz();
    });
    window.addEventListener('focus', () => {
      this.isBlurred = false;
      maybeResumeViz();
    });

    window.addEventListener('keydown', event => {
      if (event.code === 'Escape') {
        this.paused.update(p => !p);
        if (this.paused.current) {
          maybePauseViz();
        } else {
          maybeResumeViz();
        }
      }
    });
    document.addEventListener('pointerlockchange', _evt => {
      if (this.isBlurred) {
        return;
      }
      this.paused.set(!document.pointerLockElement);
    });

    document.addEventListener('mousedown', async () => {
      if ((viewMode === 'firstPerson' || viewMode === 'top-down') && !this.paused.current) {
        didManuallyLockPointer = true;
        // `unadjustedMovement` is needed to bypass mouse acceleration and prevent bad inputs
        // that happen in some cases when using high polling rate mice or something like that
        try {
          await this.renderer.domElement.requestPointerLock({ unadjustedMovement: true });
        } catch (err) {
          if (err instanceof Error && err.name === 'NotSupportedError') {
            // some browsers/operating systems don't support the `unadjustedMovement` option
            await this.renderer.domElement.requestPointerLock();
          } else {
            console.error('Failed to get pointer lock: ', err);
          }
        }
      }
    });

    this.paused.subscribe(paused => {
      if (paused) {
        maybePauseViz();
      } else {
        maybeResumeViz();
      }
    });
  };

  public respawnPlayer = () => {
    this.fpCtx?.teleportPlayer(this.spawnPos.pos, this.spawnPos.rot);
    this.onRespawnCBs.forEach(cb => cb());
  };

  public onInstakillTerrainCollision = () => {
    // TODO: Should at least play a sfx...
    this.respawnPlayer();
  };

  public registerOnRespawnCb = (cb: () => void) => this.onRespawnCBs.push(cb);
  public unregisterOnRespawnCb = (cb: () => void) => {
    const idx = this.onRespawnCBs.indexOf(cb);
    if (idx !== -1) {
      this.onRespawnCBs.splice(idx, 1);
    }
  };

  /**
   *
   * @param cb
   * @param priority the lower the priority, the earlier the callback will be called.  Defaults to 0 if not set.
   * @returns
   */
  public registerBeforeRenderCb = (
    cb: (curTimeSeconds: number, tDiffSeconds: number) => void,
    priority: number = 1
  ) => {
    this.beforeRenderCbs.push({ cb, priority });
    // sort to maintain priority order
    this.beforeRenderCbs.sort((a, b) => {
      return a.priority - b.priority;
    });
  };

  public unregisterBeforeRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = this.beforeRenderCbs.findIndex(entry => entry.cb === cb);
    if (idx !== -1) {
      this.beforeRenderCbs.splice(idx, 1);
    }
  };

  public registerAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) =>
    this.afterRenderCbs.push(cb);
  public unregisterAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = this.afterRenderCbs.indexOf(cb);
    if (idx !== -1) {
      this.afterRenderCbs.splice(idx, 1);
    }
  };

  public get destroyed() {
    return this.isDestroyed;
  }

  public onDestroy = () => {
    if (this.isDestroyed) {
      console.error('Tried to destroy already destroyed viz');
    }

    if ((window as any).recordPos) {
      (window as any).lastPos = (window as any).recordPos();
    }
    this.renderer.dispose();
    this.beforeRenderCbs.length = 0;
    this.afterRenderCbs.length = 0;

    if (this.animateHandle) {
      cancelAnimationFrame(this.animateHandle);
    }

    this.resizeCbs.length = 0;
    this.collisionWorldLoadedCbs.length = 0;
    this.distanceSwapEntries.length = 0;
    this.renderOverride = null;

    window.removeEventListener('resize', this.onWindowResize);

    this.scene.traverse(o => {
      if (o instanceof THREE.Mesh) {
        o.geometry?.dispose();
        if (Array.isArray(o.material)) {
          o.material.forEach(m => m.dispose());
        } else {
          o.material?.dispose();
        }
      }
    });

    this.inlineConsole?.destroy();

    console.clear();

    this.isDestroyed = true;
  };
}

export const initViz = (
  container: HTMLElement,
  {
    paused,
    popUpCalled,
    sceneName: providedSceneName = Conf.DefaultSceneName,
    vizCb,
  }: {
    paused: TransparentWritable<boolean>;
    popUpCalled: TransparentWritable<PopupScreenFocus>;
    sceneName?: string;
    vizCb: (viz: Viz, vizConfig: TransparentWritable<Conf.VizConfig>, sceneConf: SceneConfig) => void;
  }
) => {
  initSentry();

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }

  const viz = new Viz(paused, popUpCalled, sceneDef);
  (window as any).viz = viz;
  (window as any).THREE = THREE;

  container.appendChild(viz.renderer.domElement);
  container.appendChild(viz.stats.dom);

  const { sceneName, sceneLoader: getSceneLoader, gltfName: providedGLTFName, extension = 'gltf' } = sceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  let loader = new GLTFLoader().setPath('/');
  if (sceneDef.needsDraco) {
    const dracoLoader = new DRACOLoader().setDecoderPath(
      'https://www.gstatic.com/draco/versioned/decoders/1.5.6/'
    );
    loader = loader.setDRACOLoader(dracoLoader);
  }

  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    if (viz.destroyed) {
      return;
    }
    providedSceneName = providedSceneName.toLowerCase();

    const scene = sceneName
      ? gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group()
      : new THREE.Group();

    const [sceneLoader, vizConfig] = await Promise.all([getSceneLoader(), Conf.getVizConfig()]);
    viz.vizConfig = vizConfig;
    applyGraphicsSettings(viz, vizConfig.current.graphics);
    applyAudioSettings(vizConfig.current.audio);
    setDefaultDistanceAmpParams(null);
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene, vizConfig.current)) ?? {}),
    };
    viz.sceneConf = sceneConf;
    const rawSpawnPos = sceneConf.locations[sceneConf.spawnLocation];
    viz.spawnPos = {
      pos: Array.isArray(rawSpawnPos.pos)
        ? new THREE.Vector3(rawSpawnPos.pos[0], rawSpawnPos.pos[1], rawSpawnPos.pos[2])
        : rawSpawnPos.pos,
      rot: Array.isArray(rawSpawnPos.rot)
        ? new THREE.Vector3(rawSpawnPos.rot[0], rawSpawnPos.rot[1], rawSpawnPos.rot[2])
        : rawSpawnPos.rot,
    };
    vizCb(viz, vizConfig, sceneConf);

    if (sceneConf.viewMode.type === 'top-down') {
      viz.camera.fov = sceneConf.viewMode.cameraFOV ?? DefaultTopDownCameraFOV;
      viz.camera.updateProjectionMatrix();
    }

    if (sceneConf.renderOverride) {
      viz.setRenderOverride(sceneConf.renderOverride);
    }

    (window as any).locations = () => Object.keys(sceneConf.locations);
    (window as any).exportScene = () =>
      import('./helpers/gltfExport').then(({ exportScene }) => exportScene(viz.scene));

    if (sceneConf.enableInventory) {
      // TODO: set up inventory CBs
    }

    viz.initPauseHandlers(sceneConf.viewMode.type);
    initCustomKeyHandlers(sceneConf.customControlsEntries);

    viz.sfxManager.setConfig(mergeDeep(buildDefaultSfxConfig(), sceneConf.sfx ?? {}));
    viz.sfxManager.setVizConfig(vizConfig);

    const initialSpawnPos = (window as any).lastPos
      ? (() => {
          const lastPos = JSON.parse((window as any).lastPos);
          return {
            pos: new THREE.Vector3(lastPos.pos[0], lastPos.pos[1], lastPos.pos[2]),
            rot: new THREE.Vector3(lastPos.rot[0], lastPos.rot[1], lastPos.rot[2]),
          };
        })()
      : viz.spawnPos;

    if (sceneConf.viewMode.type === 'firstPerson' || sceneConf.viewMode.type === 'top-down') {
      viz.registerAfterRenderCb((curTimeSeconds, tDiffSeconds) =>
        viz.sfxManager.tick(tDiffSeconds, curTimeSeconds)
      );

      if (sceneConf.viewMode.type === 'firstPerson') {
        viz.camera.rotation.setFromVector3(initialSpawnPos.rot, 'YXZ');
      } else if (sceneConf.viewMode.type === 'top-down') {
        // camera looks towards negative Y.  negative X is left, negative Z is down
        viz.camera.rotation.copy(sceneConf.viewMode.cameraRotation ?? DefaultTopDownCameraRotation);
      } else {
        sceneConf.viewMode satisfies never;
        throw new Error(`Unhandled view mode: ${(sceneConf.viewMode as any).type}`);
      }

      viz.fpCtx = await setupFirstPerson({ viz, initialSpawnPos });
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

    if (viz.fpCtx) {
      const traverseCb = (obj: THREE.Object3D) => {
        const children = obj.children;
        obj.children = [];
        if (obj instanceof THREE.Mesh) {
          viz.fpCtx!.addTriMesh(obj);
        }
        obj.children = children;
      };
      traverseCollidable(scene, traverseCb);

      for (const cb of viz.collisionWorldLoadedCbs) {
        cb(viz.fpCtx);
      }

      viz.fpCtx.optimize();
    }

    if (sceneConf.player?.mesh) {
      viz.scene.add(sceneConf.player.mesh);
    }

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
      viz.onDestroy();
      viz.fpCtx?.clearCollisionWorld();
    },
  };
};
