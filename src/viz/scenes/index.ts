import type * as THREE from 'three';
import type { Viz } from '..';
import type { SfxConfig } from '../audio/SoundEngine';
import type { VizConfig } from '../conf';
import type { DeepPartial } from '../util/util.ts';
import type { TransparentWritable } from '../util/TransparentWritable.ts';
import type { PlayerShadowParams } from '../shaders/customShader';

export interface SceneMetadata {
  title: string;
  description?: string;
  openGraph?: {
    title?: string;
    description?: string;
    images?: Array<{ url: string; alt?: string; width?: number; height?: number }>;
  };
}

export interface SceneConfigLocation {
  pos: THREE.Vector3;
  rot: THREE.Vector3;
}

export type SceneLocations = {
  [key: string]: {
    pos: THREE.Vector3 | [number, number, number];
    rot: THREE.Vector3 | [number, number, number];
  };
};

export interface PlayerMoveSpeed {
  onGround: number;
  inAir: number;
}

export interface DashChargeConfig {
  curCharges: TransparentWritable<number>;
}

export interface DashConfig {
  /**
   * Default: true
   */
  enable: boolean;
  /**
   * If not provided, dashes will be unmetered
   */
  chargeConfig?: DashChargeConfig;
  dashMagnitude: number;
  minDashDelaySeconds: number;
  useExternalVelocity?: boolean;
  sfx?: {
    play?: boolean;
    name?: string;
    gainDb?: number;
  };
}

export interface CustomControlsEntry {
  label: string;
  key: string;
  action: (event?: KeyboardEvent) => void;
}

export type ViewMode =
  | { type: 'firstPerson' }
  | { type: 'orbit'; pos: THREE.Vector3; target: THREE.Vector3 }
  | {
      type: 'top-down';
      cameraOffset?: THREE.Vector3;
      cameraRotation?: THREE.Euler;
      cameraFOV?: number;
      cameraFocusPoint?: { type: 'player' } | { type: 'fixed'; pos: THREE.Vector3 };
    }
  | {
      type: 'thirdPerson';
      /**
       * Distance from the player to the camera.  Default: 15
       */
      distance?: number;
      /**
       * Minimum polar angle (from straight up / positive Y axis) in radians.
       * Prevents camera from going straight above the player.  Default: 0.15
       */
      minPolarAngle?: number;
      /**
       * Maximum polar angle (from straight up / positive Y axis) in radians.
       * Prevents camera from going straight below the player.  Default: Math.PI - 0.15
       */
      maxPolarAngle?: number;
      /**
       * Initial polar angle when switching to this mode.  Default: Math.PI / 3 (60° from top)
       */
      initialPolarAngle?: number;
      /**
       * Initial azimuth angle when switching to this mode.  Default: Math.PI (behind player)
       */
      initialAzimuthAngle?: number;
      cameraFOV?: number;
      /**
       * Clearance (in world units) kept between the camera and any occluding surface.
       * Default: 0.25
       */
      cameraCollisionBias?: number;
      /**
       * Minimum distance the camera can be from the player when occluded.  Default: 1.0
       */
      minCameraDistance?: number;
      /**
       * Speed (units/sec) at which the camera eases back to full distance after occlusion clears.
       * Default: 8.0
       */
      cameraExtendSpeed?: number;
      /**
       * Enable scroll-wheel zoom to adjust camera distance at runtime.
       * Default: false
       */
      zoomEnabled?: boolean;
      /**
       * Maximum distance the camera can be zoomed out to.
       * Default: same as `distance`
       */
      maxZoomDistance?: number;
      /**
       * Minimum distance the camera can be zoomed in to.
       * Set to 0 to allow seamless first-person transition.
       * Default: 0
       */
      minZoomDistance?: number;
      /**
       * How fast the scroll wheel adjusts distance (world units per scroll step).
       * Default: 2.0
       */
      zoomSpeed?: number;
      /**
       * Distance at which the FOV begins transitioning between first-person and
       * third-person values during zoom.  Only relevant when `zoomEnabled` is true.
       * Default: 3.0
       */
      fovTransitionDistance?: number;
    };

export interface SceneConfig {
  viewMode?: ViewMode;
  locations: SceneLocations;
  spawnLocation: string;
  /**
   * If true, the current position in the world of the player will be displayed
   */
  debugPos?: boolean;
  debugCamera?: boolean;
  /**
   * If true, the name of the object at the center of the screen will be displayed
   */
  debugTarget?: boolean;
  /**
   * If true, the player's movement and collision world state will be displayed
   */
  debugPlayerKinematics?: boolean;
  gravity?: number;
  /**
   * Shapes the gravity curve during airborne movement to allow asymmetric jump arcs.
   *
   * Effective gravity is scaled by a multiplier that varies with vertical velocity:
   *  - When `|verticalVelocity|` is large and positive (rising fast): `riseMultiplier` applies
   *  - When `|verticalVelocity|` is near zero (apex of jump): `apexMultiplier` applies
   *  - When `|verticalVelocity|` is large and negative (falling fast): `fallMultiplier` applies
   *
   * Transitions between zones are smoothed by `kneeWidth` to avoid jarring acceleration changes.
   * All multipliers default to 1.0 (no shaping / standard parabolic arc).
   */
  gravityShaping?: {
    /** Gravity multiplier when rising quickly. Default: 1.0 */
    riseMultiplier?: number;
    /** Gravity multiplier near the apex of a jump. Default: 1.0 */
    apexMultiplier?: number;
    /** Gravity multiplier when falling quickly. Default: 1.0 */
    fallMultiplier?: number;
    /** Vertical velocity (units/s) that defines the center of the apex zone. Default: 3.0 */
    apexThreshold?: number;
    /** Width of the smooth transition between zones (units/s). Default: 2.0 */
    kneeWidth?: number;
    /** If true, gravity shaping only applies during jumps (not when walking off ledges). Default: false */
    onlyJumps?: boolean;
  };
  /**
   * Tick rate in hertz used to determine the fixed time step for the bullet physics simulation.
   *
   * Default: 160
   */
  simulationTickRate?: number;
  player?: {
    dashConfig?: Partial<DashConfig>;
    jumpVelocity?: number;
    /** Terminal fall speed in units/s. Default: 55 */
    terminalVelocity?: number;
    /** Grace period in seconds after leaving a ledge during which the player can still jump. Default: 0 (disabled) */
    coyoteTimeSeconds?: number;
    /**
     * Over the course of a second `externalVelocityAirDampingFactor` percent of the external velocity
     * will bleed off while the player is in the air.  So vec3(0.5, 0.5, 0.5) means that 50% of external
     * velocity will be lost every second while in the air.
     */
    externalVelocityAirDampingFactor?: THREE.Vector3;
    /**
     * Over the course of a second `externalVelocityGroundDampingFactor` percent of the external velocity
     * will bleed off while the player is on the ground.  So vec3(0.5, 0.5, 0.5) means that 50% of external
     * velocity will be lost every second while on the ground.
     */
    externalVelocityGroundDampingFactor?: THREE.Vector3;
    colliderSize?: { height: number; radius: number };
    playerColliderShape?: 'capsule' | 'cylinder' | 'sphere';
    moveSpeed?: { onGround: number; inAir: number };
    stepHeight?: number;
    /** Maximum slope angle in radians that the player can walk up. Default: 0.8 */
    maxSlopeRadians?: number;
    /** Maximum penetration depth for the character controller. Default: 0.075 */
    maxPenetrationDepth?: number;
    /**
     * When set, surfaces steeper than `minAngle` radians cause the player to slide downhill.
     *
     * Slide speed scales linearly from 0 at `minAngle` to `maxSpeed` (units/s) at `maxSlopeRadians`.
     *
     * Disabled by default.
     */
    slopeSlide?: {
      minAngle: number;
      maxSpeed: number;
    };
    /** Minimum delay in seconds between consecutive jumps. Default: 0.25 */
    minJumpDelaySeconds?: number;
    oobYThreshold?: number;
    /**
     * If provided, this mesh will be added to the world and moved in sync with the player.  This is not
     * usually needed in `firstPerson` view mode, but is useful for `top-down` mode.
     */
    mesh?: THREE.Mesh;
    /** Renders a circular shadow beneath the player on custom shader materials. */
    playerShadow?: PlayerShadowParams;
  };
  renderOverride?: (timeDiffSeconds: number) => void;
  enableInventory?: boolean;
  sfx?: DeepPartial<SfxConfig>;
  legacyLights?: boolean;
  customControlsEntries?: CustomControlsEntry[];
  /**
   * Default true.  If true, the scene will teleport the player back to the location they were at
   * when reloading after changing graphics settings.
   *
   * This should be set to false for timed or stateful scenes where the player should not be able to
   * start at arbitrary points in the scene.
   */
  goBackOnLoad?: boolean;
  /**
   * If provided, player movement is disabled until this promise resolves. Use this to gate
   * the player from moving until async scene loading (textures, materials, geoscript) is complete.
   */
  loadingComplete?: Promise<void>;
}

type MaybePromise<T> = T | Promise<T>;

export interface SceneDef {
  /**
   * The name of the scene in the Blender file to load
   */
  sceneName: string | null;
  sceneLoader: () => MaybePromise<
    (viz: Viz, loadedWorld: THREE.Group, config: VizConfig, userData?: any) => MaybePromise<SceneConfig>
  >;
  metadata: SceneMetadata;
  gltfName?: string | null;
  extension?: 'gltf' | 'glb';
  needsDraco?: boolean;
  legacyLights?: boolean;
  /**
   * When true, `loadLevelDef` is called automatically by the framework.
   */
  useSceneDef?: boolean;
  /**
   * When false, the audio engine (AudioWorklet, wasm, sample fetches) is not
   * initialized for this scene.  Use for non-game scenes (e.g. the geoscript
   * playground) that have no sfx or spatial audio.  Default: true.
   */
  audio?: boolean;
}
