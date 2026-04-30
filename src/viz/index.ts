import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module.js';
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { initSentry } from 'src/sentry';
import { buildDefaultSfxConfig, SfxManager } from './audio/SfxManager';
import { getAmmoJS, BulletPhysics } from './collision';
import {
  FlightPlayer,
  RecorderEventType,
  fetchReplayForPlay,
  preFetchFlightRecorderWasm,
} from './flightRecorder';
import * as Conf from './conf';
import { InlineConsole } from './helpers/inlineConsole';
import { initPlayerKinematicsDebugger } from './helpers/playerKinematicsDebugger/playerKinematicsDebuggerInit.svelte.ts';
import { initEulerDebugger, initPosDebugger } from './helpers/posDebugger';
import { initTargetDebugger } from './helpers/targetDebugger';
import { Inventory } from './inventory/Inventory';
import { type SceneConfig, type SceneDef, ScenesByName, type ViewMode } from './scenes';
import { buildDefaultSceneConfig, DefaultTopDownCameraFOV } from './sceneDefaults';
import { DefaultTopDownCameraRotation } from './clientDefaults';
import type { CameraController } from './cameraController';
import {
  resetCustomShaderGlobals,
  getPlayerShadowUniforms,
  precompileOcclusionShaderVariants,
} from './shaders/customShader';
import { clearPhysicsBinding } from './util/physics';
import { clamp, delay, mergeDeep, mix, type PopupScreenFocus } from './util/util.ts';
import type { BtPairCachingGhostObject } from 'src/ammojs/ammoTypes.ts';
import { rwritable, type TransparentWritable } from './util/TransparentWritable.ts';
import { buildEasingFn, EasingFnType } from './util/easingFns.ts';
import type { Unsubscriber } from 'svelte/store';
import { unmount } from 'svelte';
import type { OrbitControls } from 'three/examples/jsm/Addons.js';
import { LoadOrbitControls } from './preloadCache';
import { loadLevelDef, type LevelLoadHandle } from './levelDef/loadLevelDef';
import { GeoscriptExecutor } from 'src/geoscript/geoscriptExecutor';
import type { LevelDef } from './levelDef/types';

export interface PostprocessingController {
  setGamma(value: number): void;
  readonly hasFinalPass: boolean;
  readonly emissiveBypassPass: { addBypassMesh(mesh: THREE.Mesh): void } | null;
}

export interface FpPlayerStateGetters {
  getVerticalVelocity: () => number;
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
  const { OrbitControls } = await LoadOrbitControls.get();
  const controls = new OrbitControls(camera, canvas);
  controls.enableDamping = true;
  controls.dampingFactor = 0.1;
  camera.position.set(pos.x, pos.y, pos.z);
  controls.target.set(target.x, target.y, target.z);
  controls.update();

  (window as any).getView = () =>
    console.log({ pos: camera.position.toArray(), target: controls.target.toArray() });
  (window as any).recordPos = () =>
    JSON.stringify({ pos: camera.position.toArray(), target: controls.target.toArray() });

  return controls;
};

export const applyGraphicsSettings = (viz: Viz, graphics: Conf.GraphicsSettings) => {
  viz.camera.fov = graphics.fov;
  viz.camera.updateProjectionMatrix();
  viz.setStatsEnabled(graphics.showFPSStats);
  viz.postprocessingController?.setGamma(graphics.gamma);
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
  public stats: Stats | null = null;
  public sceneName: string;
  public clock: THREE.Clock = new THREE.Clock();
  public inventory: Inventory = new Inventory();
  public sfxManager: SfxManager = new SfxManager();
  public scene: THREE.Scene = new THREE.Scene();
  /**
   * Overlay scene rendered after all postprocessing with a fresh depth buffer.
   * Use for editor gizmos and other always-on-top overlays that must not be
   * affected by fog, bloom, or tone mapping.
   */
  public overlayScene: THREE.Scene = new THREE.Scene();
  public paused: TransparentWritable<boolean>;
  public popupCalled: TransparentWritable<PopupScreenFocus>;
  public collisionWorldLoadedCbs: ((fpCtx: BulletPhysics) => void)[] = [];
  public fpCtx: BulletPhysics | undefined;
  /**
   * Persistent user-configurable settings, mostly set via the pause menu.
   */
  public vizConfig: TransparentWritable<Conf.VizConfig> = rwritable(Conf.loadVizConfig());
  public postprocessingController: PostprocessingController | null = null;
  public sceneConf!: SceneConfig;
  public keyStates: Record<string, boolean> = {};
  /**
   * Initially defined by the scene config, but can be changed at runtime.
   */
  public spawnPos!: { pos: THREE.Vector3; rot: THREE.Vector3 };
  public controlState: ControlState = { cameraControlEnabled: true, movementEnabled: true };
  /**
   * Camera controller for first-person and third-person modes.
   * Created when BulletPhysics initializes; undefined for orbit-only scenes.
   */
  public cameraController: CameraController | undefined;
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
  private physicsStartupBarrierCount = 0;
  private physicsStartupBarriersResolved = false;
  private resolvePhysicsStartupBarriers!: () => void;
  private physicsStartupBarriersPromise: Promise<void> = new Promise<void>(resolve => {
    this.resolvePhysicsStartupBarriers = () => {
      this.physicsStartupBarriersResolved = true;
      resolve();
    };
  });
  private inlineConsole =
    window.location.href.includes('localhost') &&
    !window.location.href.includes('geoscript') &&
    !window.location.href.includes('geotoy')
      ? new InlineConsole()
      : undefined;
  private customOnInstakillTerrainCollisionCb:
    | ((sensor: BtPairCachingGhostObject, mesh: THREE.Mesh | null) => void)
    | null = null;
  private didManuallyLockPointer = false;
  private pointerLockRequestInFlight = false;
  private clockStopTime = 0;
  private unsubscribePauseStateChange: Unsubscriber | null = null;
  private lastPauseState: boolean | null = null;
  private customKeyEventMap = new Map<string, () => void>();
  public levelLoadHandle: LevelLoadHandle | null = null;

  constructor(
    paused: TransparentWritable<boolean>,
    popupCalled: TransparentWritable<PopupScreenFocus>,
    sceneDef: SceneDef,
    sceneName: string
  ) {
    this.paused = paused;
    this.popupCalled = popupCalled;
    this.sceneName = sceneName;

    this.setupCameraAndRenderer(sceneDef);

    this.registerBeforeRenderCb(() => {
      for (const { mesh, baseMat, replacementMat, distance } of this.distanceSwapEntries) {
        const distanceToCamera = this.camera.position.distanceTo(mesh.position);
        if (distanceToCamera < distance) {
          mesh.material = baseMat;
        } else {
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

    // backwards compat
    if (sceneDef.legacyLights) {
      (this.renderer as any).useLegacyLights = true;
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

    // Render overlay scene (editor gizmos etc.) on top of everything,
    // bypassing fog, bloom, and tone mapping.
    if (this.overlayScene.children.length > 0) {
      const prevAutoClear = this.renderer.autoClear;
      this.renderer.autoClear = false;
      this.renderer.clearDepth();
      this.renderer.render(this.overlayScene, this.camera);
      this.renderer.autoClear = prevAutoClear;
    }

    this.afterRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    this.stats?.update();

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

  public setRenderOverride = (
    cb: ((timeDiffSeconds: number) => void) | null,
    clearPostprocessingController = true
  ) => {
    this.renderOverride = cb;
    if (clearPostprocessingController) {
      this.postprocessingController = null;
    }
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
    newViewMode: ViewMode,
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

    const playerPos = this.fpCtx!.playerStateGetters.getPlayerPos();
    const playerFeetPos = new THREE.Vector3(playerPos[0], playerPos[1], playerPos[2]);
    const playerEyePos = playerFeetPos.clone();
    playerEyePos.y += 0.5 * this.fpCtx!.playerColliderHeight;

    let endCameraPos: THREE.Vector3;
    let endCameraRot: THREE.Euler;
    let endFOV: number;

    if (newViewMode.type === 'firstPerson' || newViewMode.type === 'thirdPerson') {
      // For top-down to first person view transitions, the existing camera orientation doesn't
      // map to a meaningful FP look direction, so we use a default
      let overrideAngles: { phi: number; theta: number } | undefined;
      if (newViewMode.type === 'firstPerson' && this.sceneConf.viewMode!.type === 'top-down') {
        overrideAngles = { phi: Math.PI / 2, theta: Math.PI };
      }

      this.cameraController!.configure(newViewMode, overrideAngles);
      endCameraPos = this.cameraController!.computeIdealCameraPos(playerEyePos);
      endCameraRot = this.cameraController!.computeLookRotation();
      endFOV = this.cameraController!.currentFOV;

      // Warm the DoubleSide+BackSide program variants before the transition completes so
      // the first occlusion event in third person doesn't trigger a shader-compile hitch.
      if (newViewMode.type === 'thirdPerson' && this.sceneConf.viewMode!.type !== 'thirdPerson') {
        precompileOcclusionShaderVariants(this.scene, this.renderer, this.camera);
      }
    } else if (newViewMode.type === 'top-down') {
      this.cameraController?.deactivate();
      endCameraPos = this.fpCtx!.computeTopDownCameraPos(playerFeetPos.clone(), newViewMode);
      endCameraRot = (newViewMode.cameraRotation ?? DefaultTopDownCameraRotation).clone();
      endFOV = newViewMode.cameraFOV ?? DefaultTopDownCameraFOV;
    } else {
      throw new Error(`Unsupported dynamic view mode: ${(newViewMode as any).type}`);
    }

    if (transitionTimeSeconds === 0) {
      this.sceneConf.viewMode = newViewMode;
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

  public setStatsEnabled = (statsEnabled: boolean) => {
    if (!!statsEnabled === !!this.stats) {
      return;
    }

    if (statsEnabled) {
      this.stats = new Stats.default();
      this.stats.dom.style.position = 'absolute';
      this.stats.dom.style.top = '0px';
      this.stats.dom.id = 'viz-stats';

      const container = this.renderer.domElement.parentElement;
      if (!container) {
        console.error('canvas not attached to DOM; cannot add stats element');
        return;
      }
      container.appendChild(this.stats.dom);
    } else if (this.stats) {
      this.stats.dom.remove();
      this.stats = null;
    }
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
      (this.viewMode.type === 'firstPerson' ||
        this.viewMode.type === 'top-down' ||
        this.viewMode.type === 'thirdPerson') &&
      !document.pointerLockElement &&
      this.controlState.cameraControlEnabled &&
      this.didManuallyLockPointer
    ) {
      await this.requestPointerLock();
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
    if (evt.code === 'Escape' && this.controlState.cameraControlEnabled) {
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

    const key = `${evt.ctrlKey ? 'ctrl+' : ''}${evt.shiftKey ? 'shift+' : ''}${evt.key.toLowerCase()}`;

    this.customKeyEventMap.get(key)?.();
  };

  private handleKeyUp = (evt: KeyboardEvent) => {
    if (!this.inlineConsole?.isOpen) {
      this.keyStates[evt.code] = false;
    }
  };

  private handleMouseDown = async (_evt: MouseEvent) => {
    if (
      (this.viewMode.type === 'firstPerson' ||
        this.viewMode.type === 'top-down' ||
        this.viewMode.type === 'thirdPerson') &&
      !this.paused.current &&
      this.controlState.cameraControlEnabled
    ) {
      this.didManuallyLockPointer = true;
      await this.requestPointerLock();
    }
  };

  private handlePointerLockChange = (_evt: Event) => {
    this.pointerLockRequestInFlight = false;
    if (this.isBlurred || !this.controlState.cameraControlEnabled) {
      return;
    }
    this.paused.set(!document.pointerLockElement);
  };

  private isBenignPointerLockError(err: unknown): boolean {
    return (
      err instanceof Error &&
      (err.name === 'NotAllowedError' ||
        err.name === 'WrongDocumentError' ||
        err.name === 'InUseAttributeError' ||
        err.name === 'InvalidStateError')
    );
  }

  private canRequestPointerLock(): boolean {
    return (
      typeof document !== 'undefined' &&
      document.visibilityState === 'visible' &&
      document.hasFocus() &&
      !!document.body &&
      document.body.isConnected
    );
  }

  private async requestPointerLock() {
    if (document.pointerLockElement || this.pointerLockRequestInFlight || !this.canRequestPointerLock()) {
      return;
    }

    this.pointerLockRequestInFlight = true;
    try {
      // `unadjustedMovement` is needed to bypass mouse acceleration and prevent bad inputs
      // that happen in some cases when using high polling rate mice or something like that
      await document.body.requestPointerLock({ unadjustedMovement: true });
    } catch (err) {
      if (err instanceof Error && err.name === 'NotSupportedError') {
        try {
          await document.body.requestPointerLock();
        } catch (fallbackErr) {
          if (!this.isBenignPointerLockError(fallbackErr)) {
            console.error('Failed to get pointer lock: ', fallbackErr);
          }
        }
      } else if (!this.isBenignPointerLockError(err)) {
        console.error('Failed to get pointer lock: ', err);
      }
    } finally {
      this.pointerLockRequestInFlight = false;
    }
  }

  private handlePauseStateChange = (paused: boolean) => {
    if (this.lastPauseState === paused) {
      return;
    }

    if (this.lastPauseState === null) {
      this.lastPauseState = paused;
    } else {
      this.recordPauseStateTransition(paused);
      this.lastPauseState = paused;
    }

    if (paused) {
      this.maybePauseViz();
    } else {
      this.maybeResumeViz();
    }
  };

  private recordPauseStateTransition = (paused: boolean) => {
    const fpCtx = this.fpCtx;
    if (!fpCtx || fpCtx.isReplayActive) {
      return;
    }

    if (paused) {
      fpCtx.flightRecorder.recordEvent(RecorderEventType.Pause);
      return;
    }

    fpCtx.flightRecorder.recordEvent(RecorderEventType.Unpause);
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

  public registerPhysicsStartupBarrier = (barrier: Promise<unknown>) => {
    if (this.physicsStartupBarriersResolved) {
      console.error(
        'registerPhysicsStartupBarrier called after physics startup barriers have already been resolved; this barrier will be ignored.'
      );
      return;
    }
    this.physicsStartupBarrierCount += 1;
    Promise.resolve(barrier).then(
      () => {
        this.physicsStartupBarrierCount -= 1;
        if (this.physicsStartupBarrierCount === 0) {
          this.resolvePhysicsStartupBarriers();
        }
      },
      err => {
        this.physicsStartupBarrierCount -= 1;
        if (this.physicsStartupBarrierCount === 0) {
          this.resolvePhysicsStartupBarriers();
        }
        throw err;
      }
    );
  };

  public awaitPhysicsStartupBarriers = (): Promise<void> => {
    if (this.physicsStartupBarrierCount === 0) {
      if (this.fpCtx) {
        // physics is already initialized and no barriers, so we can resolve immediately
        this.resolvePhysicsStartupBarriers();
        return this.physicsStartupBarriersPromise;
      } else {
        // we have to wait for the physics engine to be loaded anyway, so wait for that and then
        // await any barriers that might be registered in the meantime
        return new Promise<void>(resolve =>
          this.collisionWorldLoadedCbs.push(() => this.awaitPhysicsStartupBarriers().then(() => resolve()))
        );
      }
    }
    return this.physicsStartupBarriersPromise;
  };

  public registerDestroyedCb = (cb: () => void) => this.onDestroyedCbs.push(cb);

  /**
   * Shared `GeoscriptExecutor` (worker hosting the geoscript runtime + Manifold).
   * Used both by level-def loading (for geoscript/csg asset resolution) and by legacy
   * `addConvexHullMesh` (for one-shot hull computation via Manifold).
   *
   * May be seeded via {@link seedGeoscriptExecutor} with a caller-owned instance — in
   * that case the seed is reused and the seed's owner is responsible for termination.
   * Otherwise a worker is lazy-spawned on first request and torn down on viz destroy.
   */
  private geoscriptExecutor: GeoscriptExecutor | null = null;
  public seedGeoscriptExecutor = (executor: GeoscriptExecutor) => {
    this.geoscriptExecutor = executor;
  };
  public getGeoscriptExecutor = (): GeoscriptExecutor => {
    if (!this.geoscriptExecutor) {
      const executor = new GeoscriptExecutor();
      this.geoscriptExecutor = executor;
      // Only auto-spawned executors are owned by Viz; seeded ones are terminated by
      // their original owner.
      this.registerDestroyedCb(() => executor.terminate());
    }
    return this.geoscriptExecutor;
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
    this.cameraController?.destroy();

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
        clearPhysicsBinding(o, this.fpCtx!);
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
  userData?: any;
  sceneDefOverride?: SceneDef;
  /**
   * Optional pre-spawned geoscript executor.  Owned by the caller (typically
   * `[scene]/+page.svelte`), constructed at component mount so the worker boot
   * + wasm fetches overlap with GLTF loading and renderer setup.
   */
  geoscriptExecutor?: GeoscriptExecutor;
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
  {
    paused,
    popUpCalled,
    sceneName: providedSceneName = Conf.DefaultSceneName,
    vizCb,
    userData,
    sceneDefOverride,
    geoscriptExecutor,
  }: InitVizArgs
) => {
  // start loading some critical async deps as early as possible
  preFetchFlightRecorderWasm();
  getAmmoJS();

  initSentry();

  const sceneDef = sceneDefOverride ?? ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }

  const {
    sceneName,
    sceneLoader: getSceneLoader,
    gltfName: providedGLTFName,
    extension = 'gltf',
    useSceneDef = false,
  } = sceneDef;
  const useLevelDef = useSceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  const vizConfP = Conf.getVizConfig();
  const sceneLoaderP = getSceneLoader();

  const viz = new Viz(paused, popUpCalled, sceneDef, providedSceneName);
  (window as any).viz = viz;
  (window as any).THREE = THREE;

  container.appendChild(viz.renderer.domElement);

  // set the clock to 0 since there could be some time in between page load and when we
  // actually start ticking the main loop
  viz.clock.start();
  setTimeout(() => viz.animate(), 0);

  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    if (viz.destroyed) {
      return;
    }
    providedSceneName = providedSceneName.toLowerCase();

    const scene = sceneName
      ? gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group()
      : new THREE.Group();

    const vizConfig = await vizConfP;
    viz.vizConfig = vizConfig;
    applyGraphicsSettings(viz, vizConfig.current.graphics);

    if (geoscriptExecutor) {
      viz.seedGeoscriptExecutor(geoscriptExecutor);
    }
    if (useSceneDef) {
      // TODO: would be ideal to start this before we even load the glTF.  Could update the level def loading
      // code to accept the asset library glTF as a promise and just await it where needed.
      viz.levelLoadHandle = loadLevelDef(
        viz,
        scene,
        userData as LevelDef,
        vizConfig.current.graphics.quality
      );
    }
    applyAudioSettings(vizConfig.current.audio);
    resetCustomShaderGlobals();
    const sceneLoader = await sceneLoaderP;
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene, vizConfig.current, userData)) ?? {}),
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
    // FP and TP FOV is managed by the CameraController (created in BulletPhysics init)

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

    viz.registerAfterRenderCb((curTimeSeconds, tDiffSeconds) =>
      viz.sfxManager.tick(tDiffSeconds, curTimeSeconds)
    );

    if (
      sceneConf.viewMode.type === 'firstPerson' ||
      sceneConf.viewMode.type === 'top-down' ||
      sceneConf.viewMode.type === 'thirdPerson'
    ) {
      const initialSpawnPos = (window as any).lastPos
        ? (() => {
            try {
              const lastPos = JSON.parse((window as any).lastPos);
              return {
                pos: new THREE.Vector3(lastPos.pos[0], lastPos.pos[1], lastPos.pos[2]),
                rot: new THREE.Vector3(lastPos.rot[0], lastPos.rot[1], lastPos.rot[2]),
              };
            } catch (err) {
              console.warn('Failed to parse lastPos', (window as any).lastPos, err);
              return viz.spawnPos;
            }
          })()
        : viz.spawnPos;

      if (sceneConf.viewMode.type === 'firstPerson') {
        viz.camera.rotation.setFromVector3(initialSpawnPos.rot, 'YXZ');
      } else if (sceneConf.viewMode.type === 'top-down') {
        // camera looks towards negative Y.  negative X is left, negative Z is down
        viz.camera.rotation.copy(sceneConf.viewMode.cameraRotation ?? DefaultTopDownCameraRotation);
      } else if (sceneConf.viewMode.type === 'thirdPerson') {
        // camera rotation will be set on the first tick via camera.lookAt(); nothing to do here
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

    if (!useLevelDef) {
      // Legacy mode: the gltf group is the scene; add it wholesale and auto-traverse for physics + shadows.
      viz.scene.add(scene);
    }

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
      if (!useLevelDef) {
        // Legacy mode: auto-register physics for all meshes in the gltf scene group.
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
            if (obj.userData.colliderShape === 'convexHull') {
              viz.fpCtx!.addConvexHullMesh(obj);
            } else {
              viz.fpCtx!.addTriMesh(obj);
            }
          }
          obj.children = children;
        };
        traverseCollidable(scene, traverseCb);
      }

      // Ensure flight recorder WASM is loaded before physics starts ticking.
      // This guarantees no subticks are lost at the start of recording.
      viz.registerPhysicsStartupBarrier(
        viz.fpCtx.flightRecorder.init().then(() => {
          viz.fpCtx!.initFlightRecorderHeader();
          // Record initial spawn position so replays know where to teleport the player.
          const sp = viz.spawnPos;
          viz.fpCtx!.flightRecorder.setMetadataString('spawn_pos', `${sp.pos.x},${sp.pos.y},${sp.pos.z}`);
          viz.fpCtx!.flightRecorder.setMetadataString(
            'spawn_rot',
            `${sp.rot?.x ?? 0},${sp.rot?.y ?? 0},${sp.rot?.z ?? 0}`
          );
        })
      );

      // Check for ?playId= query param for generic replay loading.
      // Registered as a startup barrier so physics doesn't start ticking
      // until the replay is loaded and startReplay has been called.
      const replayPlayId = new URLSearchParams(window.location.search).get('playId');
      if (replayPlayId && viz.fpCtx) {
        const fpCtx = viz.fpCtx;
        viz.registerPhysicsStartupBarrier(
          fetchReplayForPlay(replayPlayId)
            .then(async data => {
              if (!data) {
                console.error('Replay not found');
                return;
              }
              const player = new FlightPlayer();
              if (!(await player.load(data))) {
                console.error('Failed to decode replay');
                return;
              }
              fpCtx.startReplay(player);
            })
            .catch(err => console.error('Replay load error:', err))
        );
      }

      for (const cb of viz.collisionWorldLoadedCbs) {
        cb(viz.fpCtx);
      }

      await viz.awaitPhysicsStartupBarriers();
      if (viz.destroyed || !viz.fpCtx) {
        return;
      }

      viz.fpCtx.optimize();
      viz.fpCtx.startMainGameTick();
    }

    if (sceneConf.player?.playerShadow) {
      const { playerShadowParams } = getPlayerShadowUniforms();
      const ps = sceneConf.player.playerShadow;
      playerShadowParams.set(ps.radius, ps.intensity, 0, 0);
    }

    if (sceneConf.player?.mesh) {
      viz.scene.add(sceneConf.player.mesh);
    }

    // Warm both shadow-side program variants up front for scenes that can enter third-person —
    // otherwise the first occlusion-triggered DoubleSide flip recompiles every CustomShaderMaterial
    // in the scene and causes a visible hitch.
    if (sceneConf.viewMode.type === 'thirdPerson') {
      precompileOcclusionShaderVariants(viz.scene, viz.renderer, viz.camera);
    }

    if (!useLevelDef) {
      // Legacy mode: auto-set shadow flags from gltf scene userData conventions.
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
    }

    if (sceneConf.loadingComplete) {
      viz.controlState.movementEnabled = false;

      const overlay = document.createElement('div');
      overlay.style.cssText =
        'position:fixed;inset:0;display:flex;align-items:center;justify-content:center;' +
        'background:rgba(0,0,0,0.55);color:#fff;font:16px monospace;z-index:9999;pointer-events:none;';
      overlay.textContent = 'Loading...';
      container.appendChild(overlay);

      sceneConf.loadingComplete.then(() => {
        viz.controlState.movementEnabled = true;
        overlay.remove();
      });
    }
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
