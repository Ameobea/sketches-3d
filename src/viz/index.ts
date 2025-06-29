import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module.js';
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { initSentry } from 'src/sentry';
import { buildDefaultSfxConfig, SfxManager } from './audio/SfxManager';
import { getAmmoJS, BulletPhysics } from './collision';
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
  DefaultTopDownCameraFOV,
  DefaultTopDownCameraRotation,
} from './scenes';
import { setDefaultDistanceAmpParams } from './shaders/customShader';
import { clamp, delay, mergeDeep, mix, type PopupScreenFocus } from './util/util.ts';
import type { BtPairCachingGhostObject } from 'src/ammojs/ammoTypes.ts';
import { rwritable, type TransparentWritable } from './util/TransparentWritable.ts';
import { buildEasingFn, EasingFnType } from './util/easingFns.ts';
import type { Unsubscriber } from 'svelte/store';
import { unmount } from 'svelte';
import type { OrbitControls } from 'three/examples/jsm/Addons.js';

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

  return controls;
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
  public camera!: THREE.PerspectiveCamera;
  public renderer!: THREE.WebGLRenderer;
  public stats!: Stats;
  public clock: THREE.Clock = new THREE.Clock();
  public inventory: Inventory = new Inventory();
  public sfxManager: SfxManager = new SfxManager();
  public scene: THREE.Scene = new THREE.Scene();
  public paused: TransparentWritable<boolean>;
  public popupCalled: TransparentWritable<PopupScreenFocus>;
  public collisionWorldLoadedCbs: ((fpCtx: BulletPhysics) => void)[] = [];
  public fpCtx: BulletPhysics | undefined;
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
  /**
   * Only set if view mode is 'orbit'
   */
  public orbitControls: OrbitControls | null = null;

  private resizeCbs: (() => void)[] = [];
  private onDestroyedCbs: (() => void)[] = [];
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
  private inlineConsole =
    window.location.href.includes('localhost') && !window.location.href.includes('geoscript')
      ? new InlineConsole()
      : undefined;
  private customOnInstakillTerrainCollisionCb:
    | ((sensor: BtPairCachingGhostObject, mesh: THREE.Mesh | null) => void)
    | null = null;
  private didManuallyLockPointer = false;
  private clockStopTime = 0;
  private unsubscribePauseStateChange: Unsubscriber | null = null;
  private customKeyEventMap = new Map<string, () => void>();

  constructor(
    paused: TransparentWritable<boolean>,
    popupCalled: TransparentWritable<PopupScreenFocus>,
    sceneDef: SceneDef
  ) {
    this.paused = paused;
    this.popupCalled = popupCalled;

    this.setupCameraAndRenderer(sceneDef);

    const stats = new Stats.default();
    stats.dom.style.position = 'absolute';
    stats.dom.style.top = '0px';

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

  private setupCameraAndRenderer = (sceneDef: SceneDef) => {
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
  };

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

  /**
   * @param extraEndTimeSeconds amount of time the view mode will remain at its final value before
   * the `onComplete` callback is called
   */
  public startViewModeInterpolation = (
    interpolationState: ViewModeInterpolationState,
    easingFnType: EasingFnType = EasingFnType.Linear,
    onComplete?: () => void,
    extraEndTimeSeconds = 0
  ) => {
    if (this.viewModeInterpolationState) {
      throw new Error('Already interpolating view mode');
    }
    this.viewModeInterpolationState = interpolationState;

    const easingFn = buildEasingFn(easingFnType);

    const cb = async (curTimeSeconds: number) => {
      const elapsed = curTimeSeconds - this.viewModeInterpolationState!.startTimeSecs;
      const t = easingFn(clamp(elapsed / this.viewModeInterpolationState!.durationSecs, 0, 1));

      const cameraPos = this.viewModeInterpolationState!.startCameraPos.clone().lerp(
        this.viewModeInterpolationState!.endCameraPos,
        t
      );
      // TODO: interpolate this properly taking into account angle wrapping
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
        if (extraEndTimeSeconds) {
          await delay(extraEndTimeSeconds * 1_000);
        }

        this.controlState.cameraControlEnabled = true;
        this.controlState.movementEnabled = true;
        this.viewModeInterpolationState = null;
        onComplete?.();
        this.unregisterBeforeRenderCb(cb);
      }
    };
    this.registerBeforeRenderCb(cb);
  };

  public setViewMode = (
    newViewMode: NonNullable<SceneConfig['viewMode']>,
    easingFnType: EasingFnType = EasingFnType.Linear,
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
    const endCameraPos = this.fpCtx!.computeCameraPos(
      new THREE.Vector3(playerPos[0], playerPos[1], playerPos[2]),
      newViewMode
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

    this.startViewModeInterpolation(
      {
        durationSecs: transitionTimeSeconds,
        startTimeSecs: this.clock.getElapsedTime(),
        startCameraPos: this.camera.position.clone(),
        startCameraRot: this.camera.rotation.clone(),
        startCameraFov: this.camera.fov,
        endCameraPos,
        endCameraRot,
        endCameraFov: endFOV,
      },
      easingFnType,
      () => {
        this.sceneConf.viewMode = newViewMode;
        onComplete();
      }
    );

    return completePromise;
  };

  public setSpawnPos = (pos: THREE.Vector3, rot: THREE.Vector3) => {
    this.spawnPos = { pos, rot };
  };

  private maybePauseViz = () => {
    if (!this.paused.current && !this.isBlurred) {
      return;
    }

    this.clockStopTime = this.clock.getElapsedTime();
    this.clock.stop();
  };

  public maybeResumeViz = async (forceLock = false) => {
    if (this.paused.current || this.isBlurred) {
      return;
    }

    this.clock.start();
    this.clock.elapsedTime = this.clockStopTime;

    if (forceLock) {
      this.didManuallyLockPointer = true;
    }

    if (
      (this.viewMode.type === 'firstPerson' || this.viewMode.type === 'top-down') &&
      !document.pointerLockElement &&
      this.didManuallyLockPointer
    ) {
      console.log('Requesting pointer lock');
      try {
        await document.body.requestPointerLock({ unadjustedMovement: true });
      } catch (err) {
        if (err instanceof Error && err.name === 'NotSupportedError') {
          // some browsers/operating systems do not support the `unadjustedMovement` option
          await document.body.requestPointerLock();
        } else {
          console.error('Failed to get pointer lock: ', err);
        }
      }
    }
  };

  private onBlur = () => {
    this.isBlurred = true;
    this.maybePauseViz();
  };

  private onFocus = () => {
    this.isBlurred = false;
    this.maybeResumeViz();
  };

  private handleKeyDown = (evt: KeyboardEvent) => {
    if (evt.code === 'Escape') {
      this.paused.update(p => !p);
      if (this.paused.current) {
        this.maybePauseViz();
      } else {
        this.maybeResumeViz();
      }
    }

    if (!this.inlineConsole?.isOpen) {
      this.keyStates[evt.code] = true;
    }

    if (
      evt.target instanceof HTMLInputElement ||
      evt.target instanceof HTMLTextAreaElement ||
      (evt.target instanceof HTMLElement && evt.target.getAttribute('role') === 'textbox')
    ) {
      return;
    }

    this.customKeyEventMap.get(evt.key.toLowerCase())?.();
  };

  private handleKeyUp = (evt: KeyboardEvent) => {
    if (!this.inlineConsole?.isOpen) {
      this.keyStates[evt.code] = false;
    }
  };

  private handleMouseDown = async (_evt: MouseEvent) => {
    if ((this.viewMode.type === 'firstPerson' || this.viewMode.type === 'top-down') && !this.paused.current) {
      this.didManuallyLockPointer = true;
      // `unadjustedMovement` is needed to bypass mouse acceleration and prevent bad inputs
      // that happen in some cases when using high polling rate mice or something like that
      try {
        await document.body.requestPointerLock({ unadjustedMovement: true });
      } catch (err) {
        if (err instanceof Error && err.name === 'NotSupportedError') {
          // some browsers/operating systems don't support the `unadjustedMovement` option
          await document.body.requestPointerLock();
        } else {
          console.error('Failed to get pointer lock: ', err);
        }
      }
    }
  };

  private handlePointerLockChange = (_evt: Event) => {
    if (this.isBlurred) {
      return;
    }
    this.paused.set(!document.pointerLockElement);
  };

  private handlePauseStateChange = (paused: boolean) => {
    if (paused) {
      this.maybePauseViz();
    } else {
      this.maybeResumeViz();
    }
  };

  public initEventHandlers = () => {
    window.addEventListener('blur', this.onBlur);
    window.addEventListener('focus', this.onFocus);
    window.addEventListener('keydown', this.handleKeyDown);
    window.addEventListener('keyup', this.handleKeyUp);
    document.addEventListener('pointerlockchange', this.handlePointerLockChange);
    document.addEventListener('mousedown', this.handleMouseDown);
    window.addEventListener('resize', this.onWindowResize);
    this.unsubscribePauseStateChange = this.paused.subscribe(this.handlePauseStateChange);

    if (this.sceneConf.customControlsEntries) {
      for (const { key, action } of this.sceneConf.customControlsEntries) {
        this.customKeyEventMap.set(key, action);
      }
    }
  };

  private deregisterEventHandlers = () => {
    window.removeEventListener('blur', this.onBlur);
    window.removeEventListener('focus', this.onFocus);
    window.removeEventListener('keydown', this.handleKeyDown);
    window.removeEventListener('keyup', this.handleKeyUp);
    document.removeEventListener('pointerlockchange', this.handlePointerLockChange);
    document.removeEventListener('mousedown', this.handleMouseDown);
    window.removeEventListener('resize', this.onWindowResize);
    this.unsubscribePauseStateChange?.();
  };

  public respawnPlayer = () => {
    this.fpCtx?.teleportPlayer(this.spawnPos.pos, this.spawnPos.rot);
    this.onRespawnCBs.forEach(cb => cb());
  };

  private defaultOnInstakillTerrainCollision = (
    _sensor: BtPairCachingGhostObject,
    _mesh: THREE.Mesh | null
  ) => {
    // TODO: Should at least play a sfx...
    this.respawnPlayer();
  };

  public onInstakillTerrainCollision = (sensor: BtPairCachingGhostObject, mesh: THREE.Mesh | null) => {
    if (this.customOnInstakillTerrainCollisionCb) {
      this.customOnInstakillTerrainCollisionCb(sensor, mesh);
      return;
    }

    this.defaultOnInstakillTerrainCollision(sensor, mesh);
  };

  public setOnInstakillTerrainCollisionCb = (
    cb: ((sensor: BtPairCachingGhostObject, mesh: THREE.Mesh | null) => void) | null
  ) => {
    this.customOnInstakillTerrainCollisionCb = cb;
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

  public registerDestroyedCb = (cb: () => void) => {
    this.onDestroyedCbs.push(cb);
  };

  public get destroyed() {
    return this.isDestroyed;
  }

  public get viewMode() {
    return this.sceneConf.viewMode!;
  }

  public destroy = () => {
    if (this.isDestroyed) {
      console.error('Tried to destroy already destroyed viz');
    }
    this.isDestroyed = true;

    if ((window as any).recordPos) {
      (window as any).lastPos = (window as any).recordPos();
    }

    if (this.animateHandle) {
      cancelAnimationFrame(this.animateHandle);
    }

    this.renderer.dispose();
    this.beforeRenderCbs.length = 0;
    this.afterRenderCbs.length = 0;
    this.resizeCbs.length = 0;
    this.collisionWorldLoadedCbs.length = 0;
    this.distanceSwapEntries.length = 0;
    this.renderOverride = null;

    this.deregisterEventHandlers();

    for (const cb of this.onDestroyedCbs) {
      cb();
    }

    this.scene.traverse(o => {
      if (o instanceof THREE.Mesh) {
        o.geometry?.dispose();
        if (Array.isArray(o.material)) {
          o.material.forEach(m => m.dispose());
        } else {
          o.material?.dispose();
        }
        if (o.userData.rigidBody) {
          this.fpCtx!.removeCollisionObject(o.userData.rigidBody, o.name);
        } else if (o.userData.collisionObj) {
          this.fpCtx!.removeCollisionObject(o.userData.collisionObj, o.name);
        }
      }
    });

    this.inlineConsole?.destroy();

    console.clear();

    this.fpCtx?.destroy();
  };
}

interface InitVizArgs {
  paused: TransparentWritable<boolean>;
  popUpCalled: TransparentWritable<PopupScreenFocus>;
  sceneName?: string;
  vizCb: (viz: Viz, vizConfig: TransparentWritable<Conf.VizConfig>, sceneConf: SceneConfig) => void;
}

/**
 * This is the main entrypoint for the application.  It loads the specified scene and configures the
 * engine based on its configuration.
 *
 * It also initializes the core of the engine (including functionality like input handling, physics,
 * and audio) and sets up the rendering loop.
 */
export const initViz = (
  container: HTMLElement,
  { paused, popUpCalled, sceneName: providedSceneName = Conf.DefaultSceneName, vizCb }: InitVizArgs
) => {
  initSentry();

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }

  const { sceneName, sceneLoader: getSceneLoader, gltfName: providedGLTFName, extension = 'gltf' } = sceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  const scenePromises = Promise.all([getSceneLoader(), Conf.getVizConfig()]);

  const viz = new Viz(paused, popUpCalled, sceneDef);
  (window as any).viz = viz;
  (window as any).THREE = THREE;

  container.appendChild(viz.renderer.domElement);
  container.appendChild(viz.stats.dom);

  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    if (viz.destroyed) {
      return;
    }
    providedSceneName = providedSceneName.toLowerCase();

    const scene = sceneName
      ? gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group()
      : new THREE.Group();

    const [sceneLoader, vizConfig] = await scenePromises;
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

    // these rely on `sceneConf` being set, so we initialize them here rather than when `Viz` is constructed
    viz.initEventHandlers();

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

    viz.registerAfterRenderCb((curTimeSeconds, tDiffSeconds) =>
      viz.sfxManager.tick(tDiffSeconds, curTimeSeconds)
    );

    if (sceneConf.viewMode.type === 'firstPerson' || sceneConf.viewMode.type === 'top-down') {
      if (sceneConf.viewMode.type === 'firstPerson') {
        viz.camera.rotation.setFromVector3(initialSpawnPos.rot, 'YXZ');
      } else if (sceneConf.viewMode.type === 'top-down') {
        // camera looks towards negative Y.  negative X is left, negative Z is down
        viz.camera.rotation.copy(sceneConf.viewMode.cameraRotation ?? DefaultTopDownCameraRotation);
      } else {
        sceneConf.viewMode satisfies never;
        throw new Error(`Unhandled view mode: ${(sceneConf.viewMode as any).type}`);
      }

      const Ammo = await getAmmoJS();
      viz.fpCtx = new BulletPhysics({ viz, Ammo, initialSpawnPos });
    } else if (sceneConf.viewMode.type === 'orbit') {
      setupOrbitControls(
        viz.renderer.domElement,
        viz.camera,
        sceneConf.viewMode.pos,
        sceneConf.viewMode.target
      ).then(controls => {
        viz.orbitControls = controls;
      });
    }

    viz.scene.add(scene);

    let vOffset = 0;
    const mountedElements: any[] = [];
    if (sceneConf.debugPos) {
      vOffset += 24;
      mountedElements.push(initPosDebugger(viz, container, 0));
    }
    if (sceneConf.debugCamera) {
      vOffset += 24;
      mountedElements.push(initEulerDebugger(viz, container, vOffset));
    }
    if (sceneConf.debugTarget) {
      vOffset += 24;
      mountedElements.push(initTargetDebugger(viz, container, vOffset));
    }
    if (sceneConf.debugPlayerKinematics) {
      vOffset += 24;
      mountedElements.push(initPlayerKinematicsDebugger(viz, container, vOffset));
    }

    viz.registerDestroyedCb(() => {
      for (const elem of mountedElements) {
        if (elem instanceof HTMLElement) {
          elem.remove();
        } else {
          unmount(elem);
        }
      }
    });

    if (viz.fpCtx) {
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
        if (child.userData.noCastShadow || child.userData.castShadow === false) {
          child.castShadow = false;
        }
        if (child.userData.noReceiveShadow || child.userData.receiveShadow === false) {
          child.receiveShadow = false;
        }
      }
    });

    viz.animate();
  };

  if (gltfName) {
    let loader = new GLTFLoader().setPath('/');
    if (sceneDef.needsDraco) {
      const dracoLoader = new DRACOLoader().setDecoderPath(
        'https://www.gstatic.com/draco/versioned/decoders/1.5.6/'
      );
      loader = loader.setDRACOLoader(dracoLoader);
    }

    loader.load(`${gltfName}.${extension}`, gltfLoadedCB);
  } else {
    gltfLoadedCB({ scenes: [] });
  }

  return {
    destroy() {
      viz.destroy();
    },
  };
};
