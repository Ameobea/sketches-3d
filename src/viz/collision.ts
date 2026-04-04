import * as THREE from 'three';

import type { FpPlayerStateGetters, Viz } from './index.js';
import {
  getPlayerShadowUniforms,
  getOcclusionUniforms,
  setOcclusionBackfaceRendering,
} from './shaders/customShader';
import {
  DefaultExternalVelocityAirDampingFactor,
  DefaultExternalVelocityGroundDampingFactor,
  DefaultTopDownCameraOffset,
} from './clientDefaults.js';
import {
  DefaultDashConfig,
  DefaultMoveSpeed,
  DefaultOOBThreshold,
  SoftOcclusionRevealRadius,
  SoftOcclusionRevealFade,
  SoftOcclusionEyeMargin,
} from './sceneDefaults.js';
import type { SceneConfig } from './scenes/index.js';
import { CameraController } from './cameraController.js';
import { CustomShaderMaterial } from './shaders/customShader';
import { MaterialClass } from './shaders/customShader.js';
import {
  DefaultGravity,
  DefaultJumpSpeed,
  DefaultMaxPenetrationDepth,
  DefaultMaxSlopeRadians,
  DefaultMinJumpDelaySeconds,
  DefaultPlayerColliderHeight,
  DefaultPlayerColliderRadius,
  DefaultPlayerColliderShape,
  DefaultSimulationTickRateHz,
} from './conf.js';
import AmmoWasmURL from '../ammojs/ammo.wasm.wasm?url';
import type {
  AmmoInterface,
  BtBoostZone,
  BtBroadphaseInterface,
  BtCollisionConfiguration,
  BtCollisionDispatcher,
  BtCollisionObject,
  BtCollisionShape,
  BtDiscreteDynamicsWorld,
  BtJumpPad,
  BtKinematicCharacterController,
  BtPairCachingGhostObject,
  BtRigidBody,
  BtSensor,
  BtSequentialImpulseConstraintSolver,
  BtTransform,
  BtQuaternion,
  BtVec3,
} from '../ammojs/ammoTypes';
import { ZoneEventType } from '../ammojs/ammoTypes';
import { DashManager } from './DashManager.js';
import {
  FlightRecorder,
  type FlightPlayer,
  type ReplayHeaderMismatch,
  type ReplayValidationConfig,
  type SubtickPhysicsSnapshot,
  packKeyFlags,
  RecorderEventType,
  validateReplayHeader,
} from './flightRecorder.js';
import { withWorldSpaceTransform } from './util/three.js';

// ─── Subtick Input Provider ───────────────────────────────────────────────
//
// Abstracts the source of per-subtick player input so that live gameplay and
// deterministic replay can share the same subtick execution path.

/** Per-subtick input state consumed by the shared subtick execution path. */
interface SubtickInputState {
  /** Key flags: bit 0=W, 1=S, 2=A, 3=D, 4=Space, 5=Shift */
  keyFlags: number;
  /** Camera phi angle */
  phi: number;
  /** Camera theta angle */
  theta: number;
  /** Third-person camera zoom distance (0 in first-person) */
  zoomDistance: number;
  /** Whether movement input is enabled (false = no WASD/jump/dash) */
  movementEnabled: boolean;
}

/** A single field mismatch detected during replay validation. */
interface ReplayFieldMismatch {
  field: string;
  recorded: number | boolean;
  replayed: number | boolean;
  absDiff?: number;
}

/** First-divergence report from deterministic replay validation. */
interface ReplayDivergence {
  subtickIndex: number;
  physicsTime: number;
  mismatches: ReplayFieldMismatch[];
}

/** Unpack key flags into individual boolean checks */
const keyFlagW = (f: number) => (f & 1) !== 0;
const keyFlagS = (f: number) => (f & 2) !== 0;
const keyFlagA = (f: number) => (f & 4) !== 0;
const keyFlagD = (f: number) => (f & 8) !== 0;
const keyFlagSpace = (f: number) => (f & 16) !== 0;
const keyFlagShift = (f: number) => (f & 32) !== 0;

// The deterministic replay path depends on these fields matching the live
// controller configuration; if they differ, playback is not authoritative.
const FATAL_REPLAY_HEADER_FIELDS = new Set<keyof ReplayValidationConfig>([
  'tickRateHz',
  'gravity',
  'jumpSpeed',
  'moveSpeedGround',
  'moveSpeedAir',
  'colliderHeight',
  'colliderRadius',
  'extVelAirDamping',
  'extVelGroundDamping',
  'gravityShapeRiseMult',
  'gravityShapeApexMult',
  'gravityShapeFallMult',
  'gravityShapeApexThreshold',
  'gravityShapeKneeWidth',
  'gravityShapeOnlyJumps',
  'stepHeight',
  'terminalVelocity',
  'maxSlopeRadians',
  'maxPenetrationDepth',
  'coyoteTimeSeconds',
  'minJumpDelaySeconds',
  'easyModeMovement',
  'colliderShape',
  'dashEnabled',
  'dashMagnitude',
  'minDashDelaySeconds',
  'dashUseExternalVelocity',
]);

const MAX_SUBSTEPS_PER_FRAME = 120;

// Precomputed unit circle offsets for shadow ring probes (8 angles at 45° intervals)
const SHADOW_PROBE_COS = Array.from({ length: 8 }, (_, i) => Math.cos((i / 8) * Math.PI * 2));
const SHADOW_PROBE_SIN = Array.from({ length: 8 }, (_, i) => Math.sin((i / 8) * Math.PI * 2));

let ammojs: Promise<AmmoInterface> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) {
    return ammojs;
  }

  ammojs = import('../ammojs/ammo.wasm.js').then(mod =>
    (mod as any).Ammo.apply({}, [{ locateFile: () => AmmoWasmURL }])
  );
  return ammojs;
};

export type ContactRegion =
  | {
      type: 'box';
      pos: THREE.Vector3;
      halfExtents: THREE.Vector3;
      quat?: THREE.Quaternion;
    }
  | { type: 'mesh'; mesh: THREE.Mesh; margin?: number; scale?: THREE.Vector3 }
  | { type: 'convexHull'; mesh: THREE.Mesh; scale?: THREE.Vector3 }
  | { type: 'aabb'; mesh: THREE.Mesh; scale?: THREE.Vector3 }
  | { type: 'sphere'; pos: THREE.Vector3; radius: number };

export type AddPlayerRegionContactCB = (
  region: ContactRegion,
  onEnter?: () => void,
  onLeave?: () => void,
  minPenetrationDepth?: number
) => BtPairCachingGhostObject;

interface CollisionObjectRef {
  materialClass?: MaterialClass;
}

interface ZoneCallbacks {
  onEnter?: () => void;
  onLeave?: () => void;
}

export interface SensorEntry {
  sensor: BtSensor;
  ghostObj: BtPairCachingGhostObject;
  zoneId: number;
}

export interface JumpPadEntry {
  pad: BtJumpPad;
  ghostObj: BtPairCachingGhostObject;
  zoneId: number;
}

export interface BoostZoneEntry {
  zone: BtBoostZone;
  ghostObj: BtPairCachingGhostObject;
  zoneId: number;
}

// \/ This is vital for making the physics work without bad bugs like falling through floors randomly.
//
// After deconstructing what the kinematic character controller does internally, I've worked out that it
// tries to push the player both up and down by this amount every tick of the simulation.
//
// If it's too big, the player tends to clip through geometry or stuff like that.
const DEFAULT_STEP_HEIGHT = 0.05;

export interface PhysicsTicker {
  tick(physicsTime: number, fixedDt: number): void;
}

export interface PhysicsTickerHandle {
  unregister(): void;
}

interface PhysicsTickerEntry {
  ticker: PhysicsTicker;
  mesh?: THREE.Object3D;
  body?: BtRigidBody;
  lastBodyTransform?: BtTransform;
}

interface BulletPhysicsArgs {
  viz: Viz;
  Ammo: AmmoInterface;
  initialSpawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 };
}

export class BulletPhysics {
  public Ammo: AmmoInterface;
  public viz: Viz;
  public collisionWorld!: BtDiscreteDynamicsWorld;
  public collisionConfiguration!: BtCollisionConfiguration;
  public dispatcher!: BtCollisionDispatcher;
  public broadphase!: BtBroadphaseInterface;
  public solver!: BtSequentialImpulseConstraintSolver;
  public playerController!: BtKinematicCharacterController;
  public playerGhostObject!: BtPairCachingGhostObject;
  public dashManager: DashManager;
  public playerStateGetters: FpPlayerStateGetters;
  public btvec3!: (x: number, y: number, z: number) => BtVec3;
  private jumpCbs: ((curTimeSeconds: number) => void)[] = [];
  private lastJumpPhysicsTime = 0;
  private lastGroundedPhysicsTime = 0;
  private coyoteTimeSeconds = 0;
  private moveDirection = new THREE.Vector3();
  private isWalking = false;
  private upDir = new THREE.Vector3(0, 1, 0);
  private forwardDir = new THREE.Vector3();
  private leftDir = new THREE.Vector3();
  private isFlyMode = false;
  private simulationTickRate: number;
  private nextCollisionObjectRefId = 0;
  private collisionObjectRefs: Map<number, CollisionObjectRef> = new Map();
  private nextZoneId = 1;
  private zoneCallbacks: Map<number, ZoneCallbacks> = new Map();
  private sensorEntries: SensorEntry[] = [];
  private jumpPads: JumpPadEntry[] = [];
  private boostZones: BoostZoneEntry[] = [];
  private physicsElapsedTime = 0;
  private physicsSubtickCount = 0;
  /** Fractional time remainder from the last frame, used to compute how many fixed substeps to run. */
  private localTimeRemainder = 0;
  private physicsTickerEntries: PhysicsTickerEntry[] = [];
  private scratchQuat: BtQuaternion | null = null;
  private hasStartedMainGameTick = false;
  private readonly playerEyePosScratch = new THREE.Vector3();
  /** Tracks whether backface rendering was enabled last frame to avoid redundant scene traversals. */
  private backfaceRenderingEnabled = true;
  public flightRecorder: FlightRecorder = new FlightRecorder();
  private ammoStatePtr = 0;
  private replayPlayer: FlightPlayer | null = null;
  private replaySubtickIndex = 0;
  private replayActive = false;
  private replayStartCbs: (() => void)[] = [];
  /**
   * When true, the physics loop is paused waiting for a replay to finish loading.
   * This prevents physicsElapsedTime from advancing before replay start,
   * which would desync physics-time-driven elements like spinning platforms.
   */
  private replayLoadPending = false;
  /** Tracks the first divergence found during deterministic replay validation. */
  private replayFirstDivergence: ReplayDivergence | null = null;
  /** Maximum position divergence observed so far during replay validation. */
  private replayMaxPosDivergence = 0;
  /** Subtick index of the maximum position divergence. */
  private replayMaxPosDivergenceSubtick = 0;

  private releaseReplayPlayer = () => {
    if (this.replayPlayer) {
      this.replayPlayer.destroy();
      this.replayPlayer = null;
    }
  };

  constructor({ viz, Ammo, initialSpawnPos }: BulletPhysicsArgs) {
    this.Ammo = Ammo;
    this.viz = viz;
    this.simulationTickRate = viz.sceneConf.simulationTickRate ?? DefaultSimulationTickRateHz;

    this.collisionConfiguration = new Ammo.btDefaultCollisionConfiguration();
    this.dispatcher = new Ammo.btCollisionDispatcher(this.collisionConfiguration);
    this.broadphase = new Ammo.btDbvtBroadphase();
    this.solver = new Ammo.btSequentialImpulseConstraintSolver();
    this.collisionWorld = new Ammo.btDiscreteDynamicsWorld(
      this.dispatcher,
      this.broadphase,
      this.solver,
      this.collisionConfiguration
    );

    this.initBtvec3Scratch(Ammo);

    this.setupPlayerController(initialSpawnPos);
    this.createCameraController();
    this.installMouseInputHandlers();

    this.initGlobalConsoleHelpers();

    this.dashManager = new DashManager(viz);

    if (localStorage.goBackOnLoad && localStorage.backPos) {
      (window as any).back();
      delete localStorage.goBackOnLoad;
    } else {
      this.teleportPlayer(viz.spawnPos.pos, viz.spawnPos.rot);
    }

    this.playerStateGetters = {
      getPlayerPos: () => {
        const pos = this.playerController.getPosition();
        return [pos.x(), pos.y(), pos.z()];
      },
      getVerticalVelocity: () => this.playerController.getVerticalVelocity(),
      getVerticalOffset: () => this.playerController.getVerticalOffset(),
      getIsOnGround: () => this.playerController.onGround(),
      getJumpAxis: () => {
        const jumpAxis = this.playerController.getJumpAxis();
        return [jumpAxis.x(), jumpAxis.y(), jumpAxis.z()];
      },
      getExternalVelocity: () => {
        const externalVelocity = this.playerController.getExternalVelocity();
        return [externalVelocity.x(), externalVelocity.y(), externalVelocity.z()];
      },
      getIsJumping: () =>
        this.playerController.isJumping() && this.lastJumpPhysicsTime > this.dashManager.lastDashTimeSeconds,
      getIsDashing: () =>
        this.playerController.isJumping() && this.dashManager.lastDashTimeSeconds > this.lastJumpPhysicsTime,
    };
  }

  private get gravity() {
    return this.viz.sceneConf.gravity ?? 40;
  }
  private get jumpSpeed() {
    return this.viz.sceneConf.player?.jumpVelocity ?? 20;
  }
  private get minJumpDelaySeconds() {
    return this.viz.sceneConf.player?.minJumpDelaySeconds ?? DefaultMinJumpDelaySeconds;
  }
  public get playerColliderHeight() {
    return this.viz.sceneConf.player?.colliderSize?.height ?? DefaultPlayerColliderHeight;
  }
  public get playerColliderRadius() {
    return this.viz.sceneConf.player?.colliderSize?.radius ?? DefaultPlayerColliderRadius;
  }

  private initBtvec3Scratch = (Ammo: AmmoInterface) => {
    const scratchVec: BtVec3 = new Ammo.btVector3();
    this.btvec3 = (x: number, y: number, z: number) => {
      scratchVec.setValue(x, y, z);
      return scratchVec;
    };
  };

  private setupPlayerController = (initialSpawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 }) => {
    const playerInitialTransform = new this.Ammo.btTransform();
    playerInitialTransform.setIdentity();
    playerInitialTransform.setOrigin(
      this.btvec3(
        initialSpawnPos.pos.x,
        initialSpawnPos.pos.y + this.playerColliderHeight / 2 + this.playerColliderRadius,
        initialSpawnPos.pos.z
      )
    );
    this.playerGhostObject = new this.Ammo.btPairCachingGhostObject();
    this.playerGhostObject.setWorldTransform(playerInitialTransform);
    this.Ammo.destroy(playerInitialTransform);
    this.collisionWorld
      .getBroadphase()
      .getOverlappingPairCache()
      .setInternalGhostPairCallback(new this.Ammo.btGhostPairCallback());

    const playerColliderShape = this.viz.sceneConf.player?.playerColliderShape ?? DefaultPlayerColliderShape;
    const playerShape = ((): BtCollisionShape => {
      switch (playerColliderShape) {
        case 'capsule':
          return new this.Ammo.btCapsuleShape(this.playerColliderRadius, this.playerColliderHeight);
        case 'cylinder':
          const halfExtents = this.btvec3(
            this.playerColliderRadius,
            this.playerColliderHeight / 2,
            this.playerColliderRadius
          );
          return new this.Ammo.btCylinderShape(halfExtents);
        case 'sphere':
          return new this.Ammo.btSphereShape(this.playerColliderRadius);
        default:
          playerColliderShape satisfies never;
          throw new Error(
            `Unknown player collider shape: ${playerColliderShape}. Expected 'capsule' or 'cylinder'.`
          );
      }
    })();
    this.playerGhostObject.setCollisionShape(playerShape);
    this.playerGhostObject.setCollisionFlags(16); // btCollisionObject::CF_CHARACTER_OBJECT

    const playerStepHeight = this.viz.sceneConf.player?.stepHeight ?? DEFAULT_STEP_HEIGHT;
    this.playerController = new this.Ammo.btKinematicCharacterController(
      this.playerGhostObject,
      playerShape,
      playerStepHeight,
      this.btvec3(0, 1, 0)
    );
    this.playerController.setMaxPenetrationDepth(
      this.viz.sceneConf.player?.maxPenetrationDepth ?? DefaultMaxPenetrationDepth
    );
    this.playerController.setMaxSlope(this.viz.sceneConf.player?.maxSlopeRadians ?? DefaultMaxSlopeRadians);
    this.playerController.setStepHeight(playerStepHeight);
    this.playerController.setJumpSpeed(this.viz.sceneConf.player?.jumpVelocity ?? DefaultJumpSpeed);
    if (this.viz.sceneConf.player?.terminalVelocity !== undefined) {
      this.playerController.setFallSpeed(this.viz.sceneConf.player.terminalVelocity);
    }

    this.collisionWorld.addCollisionObject(
      this.playerGhostObject,
      32, // btBroadphaseProxy::CharacterFilter
      1 | 2 // btBroadphaseProxy::StaticFilter | btBroadphaseProxy::DefaultFilter
    );
    this.collisionWorld.addAction(this.playerController);

    this.setGravity(this.viz.sceneConf.gravity ?? DefaultGravity);

    const gravityShaping = this.viz.sceneConf.gravityShaping;
    if (gravityShaping) {
      if (gravityShaping.riseMultiplier !== undefined) {
        this.playerController.setGravityShapeRiseMultiplier(gravityShaping.riseMultiplier);
      }
      if (gravityShaping.apexMultiplier !== undefined) {
        this.playerController.setGravityShapeApexMultiplier(gravityShaping.apexMultiplier);
      }
      if (gravityShaping.fallMultiplier !== undefined) {
        this.playerController.setGravityShapeFallMultiplier(gravityShaping.fallMultiplier);
      }
      if (gravityShaping.apexThreshold !== undefined) {
        this.playerController.setGravityShapeApexThreshold(gravityShaping.apexThreshold);
      }
      if (gravityShaping.kneeWidth !== undefined) {
        this.playerController.setGravityShapeKneeWidth(gravityShaping.kneeWidth);
      }
      if (gravityShaping.onlyJumps !== undefined) {
        this.playerController.setGravityShapeOnlyJumps(gravityShaping.onlyJumps);
      }
    }

    const externalVelocityAirDampingFactor =
      this.viz.sceneConf.player?.externalVelocityAirDampingFactor ?? DefaultExternalVelocityAirDampingFactor;
    const externalVelocityGroundDampingFactor =
      this.viz.sceneConf.player?.externalVelocityGroundDampingFactor ??
      DefaultExternalVelocityGroundDampingFactor;
    this.playerController.setExternalVelocityAirDampingFactor(
      this.btvec3(
        externalVelocityAirDampingFactor.x,
        externalVelocityAirDampingFactor.y,
        externalVelocityAirDampingFactor.z
      )
    );
    this.playerController.setExternalVelocityGroundDampingFactor(
      this.btvec3(
        externalVelocityGroundDampingFactor.x,
        externalVelocityGroundDampingFactor.y,
        externalVelocityGroundDampingFactor.z
      )
    );

    this.coyoteTimeSeconds = this.viz.sceneConf.player?.coyoteTimeSeconds ?? 0;
  };

  private createCameraController = () => {
    const { viz } = this;
    const viewMode = viz.sceneConf.viewMode!;

    viz.cameraController = new CameraController({
      camera: viz.camera,
      getMouseSensitivity: () => viz.vizConfig.current.controls.mouseSensitivity,
      getCameraControlEnabled: () => viz.controlState.cameraControlEnabled,
      getPointerLocked: () => document.pointerLockElement === document.body,
      getFirstPersonFOV: () => viz.vizConfig.current.graphics.fov,
      getThirdPersonXrayEnabled: () => viz.vizConfig.current.gameplay.thirdPersonXray,
      cameraRayTest: (fx, fy, fz, tx, ty, tz) =>
        this.playerController.cameraRayTest(this.collisionWorld, fx, fy, fz, tx, ty, tz),
      getLastRayHitNormal: () => ({
        x: this.playerController.getCameraRayHitNormalX(),
        y: this.playerController.getCameraRayHitNormalY(),
        z: this.playerController.getCameraRayHitNormalZ(),
      }),
    });

    if (viewMode.type === 'firstPerson' || viewMode.type === 'thirdPerson') {
      viz.cameraController.configure(viewMode);
    }
  };

  private installMouseInputHandlers = () => {
    if (window.location?.href.includes('localhost')) {
      document.body.addEventListener('mousedown', evt => {
        if (evt.button === 3) {
          (window as any).back();
        }
      });
    }
  };

  private initGlobalConsoleHelpers = () => {
    (window as any).tp = (posName: string) => {
      const location = this.viz.sceneConf.locations[posName];
      const pos = Array.isArray(location.pos)
        ? new THREE.Vector3(location.pos[0], location.pos[1], location.pos[2])
        : location.pos;
      const rot = Array.isArray(location.rot)
        ? new THREE.Vector3(location.rot[0], location.rot[1], location.rot[2])
        : location.rot;
      if (location) {
        this.teleportPlayer(pos, rot);
      } else {
        console.warn(`No location found for ${posName}`);
      }
    };
    (window as any).tpos = (x: number, y: number, z: number) =>
      this.teleportPlayer(new THREE.Vector3(x, y, z));

    (window as any).back = () => {
      const backPos = localStorage.backPos;
      if (!backPos) {
        console.warn('No back position found');
        return;
      }

      const { pos, rot } = JSON.parse(backPos);
      this.teleportPlayer(
        new THREE.Vector3(pos[0], pos[1], pos[2]),
        new THREE.Vector3(rot[0], rot[1], rot[2])
      );
    };
    (window as any).getPos = () =>
      this.viz.camera.position
        .clone()
        .sub(new THREE.Vector3(0, this.playerColliderHeight / 2, 0))
        .toArray();
    (window as any).getRot = () => this.viz.camera.rotation.toArray();
    (window as any).recordPos = () =>
      JSON.stringify({
        pos: (window as any).getPos(),
        rot: this.viz.camera.rotation.toArray().slice(0, 3),
      });
    (window as any).fly = () => this.setFlyMode();

    window.onbeforeunload = function () {
      if ((window as any).recordPos) {
        localStorage.backPos = (window as any).recordPos();
      }
    };
  };

  public startMainGameTick = () => {
    if (this.hasStartedMainGameTick) {
      return;
    }
    this.hasStartedMainGameTick = true;

    // Allocate a persistent buffer in Ammo's heap for packState (10 floats = 40 bytes)
    this.ammoStatePtr = this.Ammo._malloc(40);

    this.viz.registerBeforeRenderCb(
      (curTimeSecs, tDiffSecs) => {
        const newPlayerPos = this.updateCollisionWorld(curTimeSecs, tDiffSecs);
        if (this.viz.sceneConf.player?.mesh) {
          this.viz.sceneConf.player.mesh.position.copy(newPlayerPos);
        }
        if (this.viz.sceneConf.player?.playerShadow) {
          const feetY = newPlayerPos.y - this.playerColliderHeight / 2 - this.playerColliderRadius;
          const shadowUniforms = getPlayerShadowUniforms();
          shadowUniforms.playerShadowPos.set(newPlayerPos.x, feetY, newPlayerPos.z);

          const shadowRayMaxDist = 50;
          // Start rays from player center so they clear the surface on slopes
          const rayOriginY = newPlayerPos.y;
          const rayEndY = rayOriginY - shadowRayMaxDist;
          const px = newPlayerPos.x;
          const pz = newPlayerPos.z;
          const radius = this.viz.sceneConf.player.playerShadow.radius;

          const castShadowRay = (x: number, z: number): number => {
            const frac = this.playerController.cameraRayTest(
              this.collisionWorld,
              x,
              rayOriginY,
              z,
              x,
              rayEndY,
              z
            );
            return frac < 1.0 ? rayOriginY - frac * shadowRayMaxDist : feetY - shadowRayMaxDist;
          };

          // Center ray
          const centerReceiverY = castShadowRay(px, pz);
          const centerDrop = feetY - centerReceiverY;
          shadowUniforms.playerShadowParams.z = centerReceiverY;
          shadowUniforms.playerShadowParams.w = centerDrop;

          // 8 angles at 45° intervals, two rings (outer=radius, inner=radius/2)
          // mat4 layout (column-major): cols 0-1 = outer ring (angles 0-7), cols 2-3 = inner ring
          const ringRadii = [radius, radius * 0.5];
          const elems = shadowUniforms.psRingData.elements;
          for (let ring = 0; ring < 2; ring++) {
            const r = ringRadii[ring];
            const offset = ring * 8; // 0 for outer, 8 for inner
            for (let i = 0; i < 8; i++) {
              elems[offset + i] = castShadowRay(px + SHADOW_PROBE_COS[i] * r, pz + SHADOW_PROBE_SIN[i] * r);
            }
          }
        }

        if (this.viz.controlState.cameraControlEnabled) {
          const viewMode = this.viz.sceneConf.viewMode!;
          const thirdPersonXrayEnabled = this.viz.vizConfig.current.gameplay.thirdPersonXray;
          let needBackfaces = false;

          if (viewMode.type === 'firstPerson' || viewMode.type === 'thirdPerson') {
            this.playerEyePosScratch.set(
              newPlayerPos.x,
              newPlayerPos.y + 0.5 * this.playerColliderHeight,
              newPlayerPos.z
            );
            this.viz.cameraController!.update(this.playerEyePosScratch, tDiffSecs);

            if (thirdPersonXrayEnabled) {
              const isSoftOccluded = this.viz.cameraController!.isSoftOccluded;
              const { occlusionStart, occlusionEnd, occlusionParams } = getOcclusionUniforms();
              if (isSoftOccluded) {
                occlusionStart.copy(this.playerEyePosScratch);
                occlusionEnd.copy(this.viz.camera.position);
                occlusionParams.set(
                  SoftOcclusionRevealRadius,
                  SoftOcclusionRevealFade,
                  1,
                  SoftOcclusionEyeMargin
                );
                needBackfaces = viewMode.type === 'thirdPerson';
              } else {
                occlusionParams.set(0, 0, 0, 0);
              }
            } else {
              const { occlusionParams } = getOcclusionUniforms();
              occlusionParams.set(0, 0, 0, 0);
            }
          } else if (viewMode.type === 'top-down') {
            const cameraPos = this.computeTopDownCameraPos(newPlayerPos, viewMode);
            this.viz.camera.position.copy(cameraPos);
            const { occlusionParams } = getOcclusionUniforms();
            occlusionParams.set(0, 0, 0, 0);
          }

          if (needBackfaces !== this.backfaceRenderingEnabled) {
            setOcclusionBackfaceRendering(this.viz.scene, needBackfaces);
            this.backfaceRenderingEnabled = needBackfaces;
          }
        }
      },
      // Setting this priority ensures that the physics simulation always runs last, after all user-supplied
      // callbacks have been called.  This avoids issues where the visual positions of objects that are
      // animated by the user don't line up with the collision world positions.
      Infinity
    );
  };

  public initFlightRecorderHeader = () => {
    const conf = this.viz.sceneConf;
    const playerConf = conf.player;
    const moveSpeed = playerConf?.moveSpeed ?? DefaultMoveSpeed;
    const extAirDamp =
      playerConf?.externalVelocityAirDampingFactor ?? DefaultExternalVelocityAirDampingFactor;
    const extGndDamp =
      playerConf?.externalVelocityGroundDampingFactor ?? DefaultExternalVelocityGroundDampingFactor;
    const gs = conf.gravityShaping;
    const dashConf = { ...DefaultDashConfig, ...(playerConf?.dashConfig ?? {}) };

    this.flightRecorder.setHeader({
      tickRateHz: this.simulationTickRate,
      gravity: conf.gravity ?? DefaultGravity,
      jumpSpeed: playerConf?.jumpVelocity ?? DefaultJumpSpeed,
      moveSpeedGround: moveSpeed.onGround,
      moveSpeedAir: moveSpeed.inAir,
      colliderHeight: this.playerColliderHeight,
      colliderRadius: this.playerColliderRadius,
      extVelAirDamping: [extAirDamp.x, extAirDamp.y, extAirDamp.z],
      extVelGroundDamping: [extGndDamp.x, extGndDamp.y, extGndDamp.z],
      gravityShapeRiseMult: gs?.riseMultiplier ?? 1.0,
      gravityShapeApexMult: gs?.apexMultiplier ?? 1.0,
      gravityShapeFallMult: gs?.fallMultiplier ?? 1.0,
      gravityShapeApexThreshold: gs?.apexThreshold ?? 3.0,
      gravityShapeKneeWidth: gs?.kneeWidth ?? 2.0,
      gravityShapeOnlyJumps: gs?.onlyJumps ?? false,
      stepHeight: playerConf?.stepHeight ?? DEFAULT_STEP_HEIGHT,
      terminalVelocity: playerConf?.terminalVelocity ?? 55,
      maxSlopeRadians: playerConf?.maxSlopeRadians ?? DefaultMaxSlopeRadians,
      maxPenetrationDepth: playerConf?.maxPenetrationDepth ?? DefaultMaxPenetrationDepth,
      coyoteTimeSeconds: playerConf?.coyoteTimeSeconds ?? 0,
      minJumpDelaySeconds: playerConf?.minJumpDelaySeconds ?? DefaultMinJumpDelaySeconds,
      // Easy mode movement is always enabled.
      easyModeMovement: true,
      colliderShape: playerConf?.playerColliderShape ?? DefaultPlayerColliderShape,
      dashEnabled: dashConf.enable,
      dashMagnitude: dashConf.dashMagnitude,
      minDashDelaySeconds: dashConf.minDashDelaySeconds,
      dashUseExternalVelocity: dashConf.useExternalVelocity ?? false,
    });
  };

  public computeTopDownCameraPos = (
    newPlayerPos: THREE.Vector3,
    viewMode: Extract<NonNullable<SceneConfig['viewMode']>, { type: 'top-down' }>
  ): THREE.Vector3 => {
    switch (viewMode.cameraFocusPoint?.type) {
      case undefined:
      case null:
      case 'player':
        return newPlayerPos.clone().add(viewMode.cameraOffset ?? DefaultTopDownCameraOffset);
      case 'fixed':
        return viewMode.cameraFocusPoint.pos.clone().add(viewMode.cameraOffset ?? DefaultTopDownCameraOffset);
      default:
        viewMode.cameraFocusPoint satisfies never;
        throw new Error('Unknown camera focus point type');
    }
  };

  /**
   * Must be called every subtick (unconditionally) so that coyote time
   * tracking stays up-to-date even when the player isn't pressing Space.
   */
  private updateGroundedTime = () => {
    if (this.playerController.onGround()) {
      this.lastGroundedPhysicsTime = this.physicsElapsedTime;
    }
  };

  private getDashDir = (
    viewModeType: 'firstPerson' | 'top-down' | 'thirdPerson',
    cameraDir: THREE.Vector3
  ) => {
    switch (viewModeType) {
      case 'firstPerson':
      case 'thirdPerson':
        return cameraDir;
      case 'top-down':
        return this.moveDirection.clone().lerp(this.upDir, 0.5).normalize();
      default:
        viewModeType satisfies never;
        throw new Error(`Unknown view mode: ${viewModeType}`);
    }
  };

  // ─── Shared subtick execution path ──────────────────────────────────────
  //
  // Both live gameplay and deterministic replay drive the same physics logic
  // through these methods using SubtickInputState.
  //
  /**
   * Build a SubtickInputState from the current live input sources.
   * This is the live-play counterpart to reading recorded subtick data during replay.
   */
  private buildLiveInput = (): SubtickInputState => ({
    keyFlags: packKeyFlags(this.viz.keyStates),
    // Math.fround quantizes f64 → f32 so that the values used during live play
    // are byte-identical to what the flight recorder stores (which uses Float32Array).
    // Without this, deterministic replay re-derives walk direction from the f32-rounded
    // theta/phi, producing a slightly different result that accumulates into drift.
    phi: Math.fround(this.viz.cameraController?.angles.phi ?? 0),
    theta: Math.fround(this.viz.cameraController?.angles.theta ?? 0),
    zoomDistance: Math.fround(this.viz.cameraController?.targetZoomDistance ?? 0),
    movementEnabled: this.viz.controlState.movementEnabled,
  });

  /**
   * Compute the movement direction vector from input state and camera orientation.
   */
  private computeMoveDirectionFromInput = (input: SubtickInputState) => {
    this.moveDirection.set(0, 0, 0);
    if (!input.movementEnabled) return;

    const viewMode = this.getViewMode();
    switch (viewMode.type) {
      case 'firstPerson':
      case 'thirdPerson': {
        // Derive forward/left from camera angles rather than reading camera.getWorldDirection
        // which depends on the Three.js camera having been updated this frame.
        //
        // The camera controller uses spherical coordinates where:
        //   phi   = polar angle from +Y axis (PI/2 = horizontal)
        //   theta = azimuth angle around Y axis
        //
        // The camera's Euler rotation is (phi - PI/2, theta, 0) in YXZ order.
        // The horizontal forward direction depends only on theta (azimuth).
        const sinTheta = Math.sin(input.theta);
        const cosTheta = Math.cos(input.theta);
        // Forward = horizontal projection of camera world direction = (-sin(theta), 0, -cos(theta))
        this.forwardDir.set(-sinTheta, 0, -cosTheta).normalize();
        // Left = cross(up, forward) = (-cos(theta), 0, sin(theta))
        this.leftDir.set(-cosTheta, 0, sinTheta).normalize();

        if (keyFlagW(input.keyFlags)) this.moveDirection.add(this.forwardDir);
        if (keyFlagS(input.keyFlags)) this.moveDirection.sub(this.forwardDir);
        if (keyFlagA(input.keyFlags)) this.moveDirection.add(this.leftDir);
        if (keyFlagD(input.keyFlags)) this.moveDirection.sub(this.leftDir);
        break;
      }
      case 'top-down': {
        if (keyFlagW(input.keyFlags)) this.moveDirection.z += 1;
        if (keyFlagS(input.keyFlags)) this.moveDirection.z -= 1;
        if (keyFlagA(input.keyFlags)) this.moveDirection.x += 1;
        if (keyFlagD(input.keyFlags)) this.moveDirection.x -= 1;
        break;
      }
      default:
        viewMode satisfies never;
        throw new Error(`Unknown view mode: ${viewMode}`);
    }

    // Movement normalization is always applied so single-axis and diagonal input
    // share the same target magnitude.
    const magnitude = this.moveDirection.length();
    if (magnitude > 0) {
      this.moveDirection.multiplyScalar(Math.SQRT2 / magnitude);
    }
  };

  /**
   * Attempt a jump using input state (key flags + physics timing).
   */
  private tryJumpFromInput = (input: SubtickInputState): boolean => {
    if (!input.movementEnabled || !keyFlagSpace(input.keyFlags)) {
      return false;
    }

    const onGround = this.playerController.onGround();
    const canJump =
      onGround ||
      (this.coyoteTimeSeconds > 0 &&
        this.physicsElapsedTime - this.lastGroundedPhysicsTime <= this.coyoteTimeSeconds &&
        this.physicsElapsedTime - this.lastJumpPhysicsTime > this.coyoteTimeSeconds);

    if (!canJump) return false;
    if (this.physicsElapsedTime - this.lastJumpPhysicsTime <= this.minJumpDelaySeconds) return false;

    this.playerController.jump(
      this.btvec3(
        this.moveDirection.x * (this.jumpSpeed * 0.18),
        this.jumpSpeed,
        this.moveDirection.z * (this.jumpSpeed * 0.18)
      )
    );
    this.lastJumpPhysicsTime = this.physicsElapsedTime;
    this.lastGroundedPhysicsTime = -Infinity;
    for (const cb of this.jumpCbs) {
      cb(this.physicsElapsedTime);
    }
    return true;
  };

  /**
   * Attempt a dash using input state (key flags + camera angles).
   */
  private tryDashFromInput = (input: SubtickInputState): boolean => {
    if (!input.movementEnabled || !keyFlagShift(input.keyFlags)) return false;

    // Derive camera world direction from phi/theta using the same Euler convention
    // as the camera controller: rotation = (phi - PI/2, theta, 0) in YXZ order.
    const euler = new THREE.Euler(input.phi - Math.PI / 2, input.theta, 0, 'YXZ');
    const cameraDir = new THREE.Vector3(0, 0, -1).applyEuler(euler).normalize();
    const dashDir = this.getDashDir(this.getViewMode().type, cameraDir);

    // Skip the DashManager's movementEnabled check — during deterministic replay
    // movementEnabled is false (to block live input), but we've already checked
    // the recorded input.movementEnabled above.
    const dashed = this.dashManager.tryDash(this.physicsElapsedTime, this.isFlyMode, dashDir, {
      skipMovementEnabledCheck: true,
    });
    if (dashed && !this.replayActive) {
      this.flightRecorder.recordEvent(RecorderEventType.Dash, [dashDir.x, dashDir.y, dashDir.z]);
    }
    return dashed;
  };

  /**
   * Advance the physics simulation by one subtick using the provided input state.
   * This is the shared core used by both live gameplay and deterministic replay.
   *
   * Returns whether a jump was fired this subtick (needed for recording).
   */
  private advanceOneSubtick = (
    input: SubtickInputState,
    fixedTimeStep: number,
    prevOnGround: boolean
  ): { jumpFired: boolean; nowOnGround: boolean } => {
    this.physicsSubtickCount++;
    this.physicsElapsedTime = this.physicsSubtickCount * fixedTimeStep;

    // Apply camera state from input — needed so that movement direction and dash
    // direction calculations are correct for this subtick
    this.viz.cameraController?.setAngles(input.phi, input.theta);
    if (input.zoomDistance > 0) {
      this.viz.cameraController?.setTargetZoomDistance(input.zoomDistance);
    }

    // Compute movement direction from input
    this.computeMoveDirectionFromInput(input);

    // Walk SFX tracking — only during live play (not replay)
    if (!this.replayActive) {
      const wasWalking = this.isWalking;
      this.isWalking = this.moveDirection.x !== 0 || this.moveDirection.y !== 0 || this.moveDirection.z !== 0;
      if (wasWalking && !this.isWalking) {
        this.viz.sfxManager.onWalkStop();
      } else if (!wasWalking && this.isWalking) {
        this.viz.sfxManager.onWalkStart(MaterialClass.Default);
      }
    }

    // Track ground state every subtick so coyote time works
    this.updateGroundedTime();

    // Check jump and dash
    const jumpFired = this.tryJumpFromInput(input);
    if (jumpFired && !this.replayActive) {
      this.flightRecorder.recordEvent(RecorderEventType.Jump, [
        this.moveDirection.x,
        this.moveDirection.y,
        this.moveDirection.z,
      ]);
    }
    this.tryDashFromInput(input);
    this.dashManager.tick(this.physicsElapsedTime, this.playerController.onGround());

    // Apply walk direction with appropriate move speed
    const playerMoveSpeed = this.viz.sceneConf.player?.moveSpeed ?? DefaultMoveSpeed;
    const onGround = this.playerController.onGround();
    const moveSpeed = onGround && !jumpFired ? playerMoveSpeed.onGround : playerMoveSpeed.inAir;
    this.playerController.setWalkDirection(
      this.btvec3(
        this.moveDirection.x * moveSpeed,
        this.moveDirection.y * moveSpeed,
        this.moveDirection.z * moveSpeed
      )
    );

    if (jumpFired) {
      this.playerController.setOnGround(false);
    }

    // Step physics
    this.tickPhysicsTickers(fixedTimeStep);
    this.collisionWorld.substepSimulation(fixedTimeStep);

    // Record subtick snapshot (suppressed during replay to avoid corrupting the recorder)
    if (this.flightRecorder.isReady && !this.replayActive) {
      this.playerController.packState(this.ammoStatePtr);
      this.flightRecorder.recordSubtick(
        this.ammoStatePtr,
        this.Ammo.HEAPF32,
        this.moveDirection.x * moveSpeed,
        this.moveDirection.y * moveSpeed,
        this.moveDirection.z * moveSpeed,
        input.phi,
        input.theta,
        input.zoomDistance,
        input.keyFlags
      );
    }

    this.drainZoneEvents();

    // OOB check — teleport player back to spawn if they fall below the threshold
    const oobThreshold = this.viz.sceneConf.player?.oobYThreshold ?? DefaultOOBThreshold;
    const playerY = this.playerController.getPosition().y();
    if (playerY <= oobThreshold) {
      if (!this.replayActive) {
        const sp = this.viz.spawnPos;
        this.flightRecorder.recordEvent(RecorderEventType.OOBRespawn, [
          sp.pos.x,
          sp.pos.y,
          sp.pos.z,
          sp.rot?.x ?? 0,
          sp.rot?.y ?? 0,
          sp.rot?.z ?? 0,
        ]);
      }
      this.viz.respawnPlayer();
    }

    // Landing SFX
    const nowOnGround = this.playerController.onGround();
    if (!prevOnGround && nowOnGround) {
      const landedOnObjectIx: number = this.playerController.getFloorUserIndex();
      const landedOnObject = this.collisionObjectRefs.get(landedOnObjectIx);
      const materialClass = landedOnObject?.materialClass ?? MaterialClass.Default;
      this.viz.sfxManager.onPlayerLand(materialClass);
    }

    return { jumpFired, nowOnGround };
  };

  public setGravity = (gravity: number) => {
    this.collisionWorld.setGravity(this.btvec3(0, -gravity, 0));
    this.playerController.setGravity(this.btvec3(0, -gravity, 0));
  };

  public setFlyMode = (newIsFlyMode?: boolean) => {
    if (newIsFlyMode === this.isFlyMode) {
      return;
    }

    this.isFlyMode = newIsFlyMode ?? !this.isFlyMode;
    this.setGravity(this.isFlyMode ? 0 : this.gravity);
  };

  private getViewMode = (): Extract<
    NonNullable<SceneConfig['viewMode']>,
    { type: 'firstPerson' | 'top-down' | 'thirdPerson' }
  > => {
    const viewMode = this.viz.sceneConf.viewMode!;
    if (viewMode.type !== 'firstPerson' && viewMode.type !== 'top-down' && viewMode.type !== 'thirdPerson') {
      throw new Error(
        `View mode must be 'firstPerson', 'top-down', or 'thirdPerson' for collision; found: '${viewMode.type}'`
      );
    }

    return viewMode;
  };

  public registerJumpCb = (cb: (curTimeSeconds: number) => void) => {
    this.jumpCbs.push(cb);
  };

  public registerReplayStartCb = (cb: () => void) => {
    this.replayStartCbs.push(cb);
  };
  public deregisterJumpCb = (cb: (curTimeSeconds: number) => void) => {
    const ix = this.jumpCbs.indexOf(cb);
    if (ix === -1) {
      throw new Error('cb not registered');
    }
    this.jumpCbs[ix] = this.jumpCbs[this.jumpCbs.length - 1];
    this.jumpCbs.pop();
  };

  /**
   * Drains the event queue from the character controller and dispatches
   * callbacks via the unified zoneCallbacks map.
   */
  private drainZoneEvents = () => {
    const numEvents = this.playerController.getNumPendingEvents();
    for (let i = 0; i < numEvents; i++) {
      const zoneId = this.playerController.getPendingEventId(i);
      const eventType = this.playerController.getPendingEventType(i);
      const cbs = this.zoneCallbacks.get(zoneId);
      if (!cbs) {
        continue;
      }

      switch (eventType) {
        case ZoneEventType.JumpPadTriggered:
          this.dashManager.resetDashGroundTouch();
          cbs.onEnter?.();
          break;
        case ZoneEventType.SensorEnter:
        case ZoneEventType.BoostZoneEnter:
          cbs.onEnter?.();
          break;
        case ZoneEventType.SensorLeave:
        case ZoneEventType.BoostZoneExit:
          cbs.onLeave?.();
          break;
      }
    }
    this.playerController.clearPendingEvents();
  };

  // ─── Replay ────────────────────────────────────────────────────────────

  private buildReplayValidationConfig = (): ReplayValidationConfig => {
    const conf = this.viz.sceneConf;
    const playerConf = conf.player;
    const moveSpeed = playerConf?.moveSpeed ?? DefaultMoveSpeed;
    const extAirDamp =
      playerConf?.externalVelocityAirDampingFactor ?? DefaultExternalVelocityAirDampingFactor;
    const extGndDamp =
      playerConf?.externalVelocityGroundDampingFactor ?? DefaultExternalVelocityGroundDampingFactor;
    const gs = conf.gravityShaping;
    const dashConf = { ...DefaultDashConfig, ...(playerConf?.dashConfig ?? {}) };
    const shapeMap = { capsule: 0, cylinder: 1, sphere: 2 } as const;
    return {
      tickRateHz: this.simulationTickRate,
      gravity: conf.gravity ?? DefaultGravity,
      jumpSpeed: playerConf?.jumpVelocity ?? DefaultJumpSpeed,
      moveSpeedGround: moveSpeed.onGround,
      moveSpeedAir: moveSpeed.inAir,
      colliderHeight: this.playerColliderHeight,
      colliderRadius: this.playerColliderRadius,
      extVelAirDamping: [extAirDamp.x, extAirDamp.y, extAirDamp.z],
      extVelGroundDamping: [extGndDamp.x, extGndDamp.y, extGndDamp.z],
      gravityShapeRiseMult: gs?.riseMultiplier ?? 1.0,
      gravityShapeApexMult: gs?.apexMultiplier ?? 1.0,
      gravityShapeFallMult: gs?.fallMultiplier ?? 1.0,
      gravityShapeApexThreshold: gs?.apexThreshold ?? 3.0,
      gravityShapeKneeWidth: gs?.kneeWidth ?? 2.0,
      gravityShapeOnlyJumps: gs?.onlyJumps ?? false,
      stepHeight: playerConf?.stepHeight ?? DEFAULT_STEP_HEIGHT,
      terminalVelocity: playerConf?.terminalVelocity ?? 55,
      maxSlopeRadians: playerConf?.maxSlopeRadians ?? DefaultMaxSlopeRadians,
      maxPenetrationDepth: playerConf?.maxPenetrationDepth ?? DefaultMaxPenetrationDepth,
      coyoteTimeSeconds: playerConf?.coyoteTimeSeconds ?? 0,
      minJumpDelaySeconds: playerConf?.minJumpDelaySeconds ?? DefaultMinJumpDelaySeconds,
      easyModeMovement: true,
      colliderShape: shapeMap[playerConf?.playerColliderShape ?? DefaultPlayerColliderShape],
      dashEnabled: dashConf.enable,
      dashMagnitude: dashConf.dashMagnitude,
      minDashDelaySeconds: dashConf.minDashDelaySeconds,
      dashUseExternalVelocity: dashConf.useExternalVelocity ?? false,
    };
  };

  /**
   * Start replay playback.
   */
  public startReplay = (player: FlightPlayer): void => {
    this.releaseReplayPlayer();
    const header = player.getHeader();
    const mismatches = validateReplayHeader(header, this.buildReplayValidationConfig());
    const formatHeaderMismatch = (m: ReplayHeaderMismatch) => {
      const diff = m.absDiff !== undefined ? ` (Δ${m.absDiff.toExponential(4)})` : '';
      return `${m.field}: replay=${m.recorded}, current=${m.current}${diff}`;
    };
    const fatalMismatches = mismatches.filter(m => FATAL_REPLAY_HEADER_FIELDS.has(m.field));
    if (fatalMismatches.length > 0) {
      player.destroy();
      throw new Error(
        `Cannot replay: incompatible physics configuration\n` +
          fatalMismatches.map(m => `  ${formatHeaderMismatch(m)}`).join('\n')
      );
    }
    if (mismatches.length > 0) {
      console.warn(
        `Replay header mismatches (${mismatches.length}):\n` +
          mismatches.map(m => `  ${formatHeaderMismatch(m)}`).join('\n')
      );
    }

    // Clear residual velocity / grounded state before replay subtick 0.
    this.reset();

    this.replayLoadPending = false;
    this.replayPlayer = player;
    this.replaySubtickIndex = 0;
    this.replayActive = true;
    this.replayFirstDivergence = null;
    this.replayMaxPosDivergence = 0;
    this.replayMaxPosDivergenceSubtick = 0;
    this.viz.controlState.movementEnabled = false;
    for (const cb of this.replayStartCbs) {
      cb();
    }
  };

  public stopReplay = (): void => {
    if (this.replayFirstDivergence) {
      const d = this.replayFirstDivergence;
      const fields = d.mismatches.map(m => {
        const diff = m.absDiff !== undefined ? ` (Δ${m.absDiff.toExponential(4)})` : '';
        return `    ${m.field}: recorded=${m.recorded}, replayed=${m.replayed}${diff}`;
      });
      console.warn(
        `[replay-validation] Summary: first divergence at subtick ${d.subtickIndex} ` +
          `(t=${d.physicsTime.toFixed(4)}s), ` +
          `max pos drift=${this.replayMaxPosDivergence.toExponential(4)} at subtick ${this.replayMaxPosDivergenceSubtick}\n` +
          `  First divergence fields:\n${fields.join('\n')}`
      );
    }

    this.replayActive = false;
    this.releaseReplayPlayer();
    this.viz.controlState.movementEnabled = true;
  };

  /**
   * Pause physics ticking while waiting for a replay to load.
   * Call this before starting an async replay fetch so that physics time
   * doesn't advance and desync platforms/timers.
   */
  public setReplayLoadPending = (pending: boolean) => {
    this.replayLoadPending = pending;
  };

  public get isReplayActive(): boolean {
    return this.replayActive;
  }

  /**
   * Returns the new position of the player.
   */
  public updateCollisionWorld = (curTimeSeconds: number, tDiffSeconds: number): THREE.Vector3 => {
    // While waiting for a replay to load, freeze physics so that
    // physicsElapsedTime doesn't advance and desync timed elements.
    if (this.replayLoadPending) {
      const t = this.playerGhostObject.getWorldTransform().getOrigin();
      return new THREE.Vector3(t.x(), t.y(), t.z());
    }

    if (this.replayActive) {
      return this.updateCollisionWorldDeterministicReplay(tDiffSeconds);
    }
    this.playerController.resetForcedRotation();
    const input = this.buildLiveInput();

    const fixedTimeStep = 1 / this.simulationTickRate;
    const maxSubSteps = MAX_SUBSTEPS_PER_FRAME;

    let prevOnGround = this.playerController.onGround();
    const numSubSteps = this.computeSubstepCount(tDiffSeconds, fixedTimeStep, maxSubSteps);
    for (let i = 0; i < numSubSteps; i++) {
      const result = this.advanceOneSubtick(input, fixedTimeStep, prevOnGround);
      prevOnGround = result.nowOnGround;
    }
    this.syncPhysicsTickerVisuals();

    const newPlayerTransform = this.playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  private validateSubtickAgainstSnapshot = (
    subtickIndex: number,
    recorded: SubtickPhysicsSnapshot
  ): ReplayDivergence | null => {
    this.playerController.packState(this.ammoStatePtr);
    const heap = this.Ammo.HEAPF32;
    const base = this.ammoStatePtr / 4;

    // Read current state from Ammo heap (same layout as packState: 10 floats)
    const curPosX = heap[base];
    const curPosY = heap[base + 1];
    const curPosZ = heap[base + 2];
    const curExtVelX = heap[base + 3];
    const curExtVelY = heap[base + 4];
    const curExtVelZ = heap[base + 5];
    const curVertVel = heap[base + 6];
    const curVertOffset = heap[base + 7];

    // Flags are bitcast u32 at [8]
    const flagsU32 = new DataView(heap.buffer, this.ammoStatePtr + 32, 4).getUint32(0, true);
    const curOnGround = (flagsU32 & 1) !== 0;
    const curIsJumping = (flagsU32 & 2) !== 0;

    // Floor user index is bitcast i32 at [9]
    const curFloorIdx = new DataView(heap.buffer, this.ammoStatePtr + 36, 4).getInt32(0, true);

    const mismatches: ReplayFieldMismatch[] = [];
    const TOL = 1e-4;

    const checkFloat = (field: string, rec: number, cur: number) => {
      const diff = Math.abs(rec - cur);
      if (diff > TOL) {
        mismatches.push({ field, recorded: rec, replayed: cur, absDiff: diff });
      }
    };
    const checkBool = (field: string, rec: boolean, cur: boolean) => {
      if (rec !== cur) {
        mismatches.push({ field, recorded: rec, replayed: cur });
      }
    };
    const checkInt = (field: string, rec: number, cur: number) => {
      if (rec !== cur) {
        mismatches.push({ field, recorded: rec, replayed: cur, absDiff: Math.abs(rec - cur) });
      }
    };

    checkFloat('pos.x', recorded.pos[0], curPosX);
    checkFloat('pos.y', recorded.pos[1], curPosY);
    checkFloat('pos.z', recorded.pos[2], curPosZ);
    checkFloat('extVel.x', recorded.externalVel[0], curExtVelX);
    checkFloat('extVel.y', recorded.externalVel[1], curExtVelY);
    checkFloat('extVel.z', recorded.externalVel[2], curExtVelZ);
    checkFloat('verticalVel', recorded.verticalVel, curVertVel);
    checkFloat('verticalOffset', recorded.verticalOffset, curVertOffset);
    checkBool('onGround', recorded.onGround, curOnGround);
    checkBool('isJumping', recorded.isJumping, curIsJumping);
    checkInt('floorUserIndex', recorded.floorUserIndex, curFloorIdx);

    if (mismatches.length === 0) return null;
    return { subtickIndex, physicsTime: this.physicsElapsedTime, mismatches };
  };

  /**
   * Deterministic replay: drives the game through the same gameplay logic as live
   * play, using recorded per-subtick input (key flags + camera angles) instead of
   * live input.  Jump/dash decisions are made by the same code paths as live play
   * rather than being force-injected from recorded events.
   *
   * After each substep, validates the resulting physics state against the recorded
   * snapshot and tracks the first divergence and maximum position drift.
   */
  private updateCollisionWorldDeterministicReplay = (tDiffSeconds: number): THREE.Vector3 => {
    const player = this.replayPlayer!;
    this.playerController.resetForcedRotation();

    const fixedTimeStep = 1 / this.simulationTickRate;
    const maxSubSteps = MAX_SUBSTEPS_PER_FRAME;

    const numSubSteps = this.computeSubstepCount(tDiffSeconds, fixedTimeStep, maxSubSteps);
    let prevOnGround = this.playerController.onGround();
    for (let i = 0; i < numSubSteps; i++) {
      if (this.replaySubtickIndex >= player.subtickCount) {
        this.stopReplay();
        break;
      }

      // Build input state from the recorded subtick data
      const subtick = player.getSubtick(this.replaySubtickIndex);
      const input: SubtickInputState = {
        keyFlags: subtick.keyFlags,
        phi: subtick.phi,
        theta: subtick.theta,
        zoomDistance: subtick.zoomDistance,
        // Movement is always enabled during deterministic replay — the recorded
        // key flags already reflect what the player actually pressed
        movementEnabled: true,
      };

      const result = this.advanceOneSubtick(input, fixedTimeStep, prevOnGround);

      // ── Per-subtick validation ──
      const recorded = player.getSubtickPhysicsSnapshot(this.replaySubtickIndex);
      const divergence = this.validateSubtickAgainstSnapshot(this.replaySubtickIndex, recorded);
      if (divergence) {
        // Track first divergence
        if (!this.replayFirstDivergence) {
          this.replayFirstDivergence = divergence;
          console.warn(
            `[replay-validation] First divergence at subtick ${divergence.subtickIndex} ` +
              `(t=${divergence.physicsTime.toFixed(4)}s):`,
            divergence.mismatches
          );
        }

        // Track max position divergence
        const posMismatch = divergence.mismatches.find(m => m.field.startsWith('pos.'));
        if (posMismatch && posMismatch.absDiff !== undefined) {
          // Compute full 3D position distance for this subtick
          const dx = recorded.pos[0] - this.playerController.getPosition().x();
          const dy = recorded.pos[1] - this.playerController.getPosition().y();
          const dz = recorded.pos[2] - this.playerController.getPosition().z();
          const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
          if (dist > this.replayMaxPosDivergence) {
            this.replayMaxPosDivergence = dist;
            this.replayMaxPosDivergenceSubtick = this.replaySubtickIndex;
          }
        }
      }

      prevOnGround = result.nowOnGround;
      this.replaySubtickIndex++;
    }
    this.syncPhysicsTickerVisuals();

    const newPlayerTransform = this.playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();
    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  /**
   * Resets player velocity, key states, and all cooldowns without touching
   * physics time. Use this for mid-run respawns (OOB, instakill) so the run
   * timer keeps ticking and physics-driven objects (spinners, sliders) are
   * not disturbed.
   */
  public resetPlayerState = () => {
    for (const key of Object.keys(this.viz.keyStates)) {
      this.viz.keyStates[key] = false;
    }
    this.playerController.setExternalVelocity(this.btvec3(0, 0, 0));
    this.playerController.setVerticalVelocity(0);
    this.playerController.setOnGround(false);
    this.lastJumpPhysicsTime = -Infinity;
    this.lastGroundedPhysicsTime = -Infinity;
    this.dashManager.lastDashTimeSeconds = -Infinity;
    this.dashManager.needsGroundTouch = false;
    this.playerController.resetAllCooldowns();
  };

  public reset = () => {
    this.resetPlayerState();
    this.resetPhysicsTime();
  };

  /**
   * Reset all JS-side and C++-side timing state to zero.
   * After this call, physicsElapsedTime is 0 and all cooldowns are cleared.
   */
  public resetPhysicsTime = () => {
    this.physicsSubtickCount = 0;
    this.physicsElapsedTime = 0;
    this.localTimeRemainder = 0;
    this.lastJumpPhysicsTime = -Infinity;
    this.lastGroundedPhysicsTime = -Infinity;
    this.dashManager.lastDashTimeSeconds = -Infinity;
    this.dashManager.needsGroundTouch = false;
    this.playerController.resetAllCooldowns();
  };

  /**
   * Compute how many fixed substeps to run for this frame.
   * Accumulates the frame delta into localTimeRemainder and returns the
   * number of whole fixed-timestep intervals, clamped to maxSubSteps.
   */
  private computeSubstepCount = (
    tDiffSeconds: number,
    fixedTimeStep: number,
    maxSubSteps: number
  ): number => {
    this.localTimeRemainder += tDiffSeconds;
    if (this.localTimeRemainder < fixedTimeStep) {
      return 0;
    }
    const count = Math.floor(this.localTimeRemainder / fixedTimeStep);
    const clamped = Math.min(count, maxSubSteps);
    if (count > maxSubSteps) {
      console.warn(
        `[physics] Dropping ${count - maxSubSteps} subticks after a long frame ` +
          `(requested=${count}, simulated=${maxSubSteps}, dt=${tDiffSeconds.toFixed(4)}s)`
      );
    }
    this.localTimeRemainder -= count * fixedTimeStep;
    return clamped;
  };

  /**
   * Reset the flight recorder for a new run while preserving the header.
   * Call this instead of flightRecorder.reset() directly.
   */
  public resetRecorderForNewRun = () => {
    this.flightRecorder.reset();
    this.initFlightRecorderHeader();
  };

  public getPhysicsTime = (): number => {
    return this.physicsElapsedTime;
  };

  public registerPhysicsTicker = (
    ticker: PhysicsTicker,
    opts?: { mesh?: THREE.Object3D; body?: BtRigidBody }
  ): PhysicsTickerHandle => {
    const lastBodyTransform = opts?.body ? new this.Ammo.btTransform() : undefined;
    if (lastBodyTransform && opts?.body) {
      lastBodyTransform.setIdentity();
      this.copyTransform(lastBodyTransform, opts.body.getWorldTransform());
    }

    const entry: PhysicsTickerEntry = {
      ticker,
      mesh: opts?.mesh,
      body: opts?.body,
      lastBodyTransform,
    };
    this.physicsTickerEntries.push(entry);

    return {
      unregister: () => {
        const ix = this.physicsTickerEntries.indexOf(entry);
        if (ix !== -1) {
          if (entry.lastBodyTransform) {
            this.Ammo.destroy(entry.lastBodyTransform);
          }
          this.physicsTickerEntries[ix] = this.physicsTickerEntries[this.physicsTickerEntries.length - 1];
          this.physicsTickerEntries.pop();
        }
      },
    };
  };

  private tickPhysicsTickers = (fixedTimeStep: number) => {
    for (const entry of this.physicsTickerEntries) {
      entry.ticker.tick(this.physicsElapsedTime, fixedTimeStep);
      if (!entry.body || !entry.lastBodyTransform) {
        continue;
      }
      const transform = entry.body.getWorldTransform();
      // `btKinematicCharacterController::maybeApplyFloorLock` reads these interpolation velocities.
      // Any kinematic motion path that bypasses `registerPhysicsTicker` must set equivalent velocities
      // every substep, otherwise floor-lock on moving/rotating platforms can silently break.
      this.collisionWorld.computeAndSetInterpolationVelocity(
        entry.body,
        entry.lastBodyTransform,
        transform,
        fixedTimeStep
      );
      this.copyTransform(entry.lastBodyTransform, transform);
    }
  };

  private syncPhysicsTickerVisuals = () => {
    for (const entry of this.physicsTickerEntries) {
      if (!entry.body) {
        continue;
      }

      const transform = entry.body.getWorldTransform();

      const motionState = entry.body.getMotionState();
      if (motionState) {
        motionState.setWorldTransform(transform);
      }

      if (!entry.mesh) {
        continue;
      }

      const origin = transform.getOrigin();
      entry.mesh.position.set(origin.x(), origin.y(), origin.z());

      const rot = transform.getRotation();
      entry.mesh.quaternion.set(rot.x(), rot.y(), rot.z(), rot.w());
    }
  };

  private copyTransform = (dst: BtTransform, src: BtTransform) => {
    const origin = src.getOrigin();
    dst.setOrigin(this.btvec3(origin.x(), origin.y(), origin.z()));
    if (!this.scratchQuat) {
      this.scratchQuat = new this.Ammo.btQuaternion(0, 0, 0, 1);
    }
    const rot = src.getRotation();
    this.scratchQuat.setValue(rot.x(), rot.y(), rot.z(), rot.w());
    dst.setRotation(this.scratchQuat);
  };

  public addCollisionObject = (
    shape: BtCollisionShape,
    pos: THREE.Vector3,
    quat: THREE.Quaternion = new THREE.Quaternion(),
    objRef?: CollisionObjectRef,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const transform = new this.Ammo.btTransform();
    transform.setIdentity();
    transform.setOrigin(this.btvec3(pos.x, pos.y, pos.z));
    const rot = new this.Ammo.btQuaternion(quat.x, quat.y, quat.z, quat.w);
    transform.setRotation(rot);
    this.Ammo.destroy(rot);

    // Add the object as static, so it doesn't move but still collides
    const motionState = new this.Ammo.btDefaultMotionState(transform);
    const localInertia = this.btvec3(0, 0, 0);
    const rbInfo = new this.Ammo.btRigidBodyConstructionInfo(0, motionState, shape, localInertia);
    const body: BtRigidBody = new this.Ammo.btRigidBody(rbInfo);
    switch (colliderType) {
      case 'static':
        body.setCollisionFlags(1); // btCollisionObject::CF_STATIC_OBJECT
        break;
      case 'kinematic':
        body.setCollisionFlags(2); // btCollisionObject::CF_KINEMATIC_OBJECT
        body.setActivationState(4); // DISABLE_DEACTIVATION
        break;
      default:
        colliderType satisfies never;
        throw new Error(`Unknown collider type: ${colliderType}. Expected 'static' or 'kinematic'.`);
    }

    if (objRef) {
      const refIx = this.nextCollisionObjectRefId++;
      body.setUserIndex(refIx);
      this.collisionObjectRefs.set(refIx, objRef);
    }
    this.collisionWorld.addRigidBody(body);

    this.Ammo.destroy(rbInfo);
    this.Ammo.destroy(transform);
    return body;
  };

  public removeCollisionObject = (collisionObj: BtCollisionObject, meshName?: string) => {
    this.collisionWorld.removeCollisionObject(collisionObj);

    const rigidBody = this.Ammo.btRigidBody.prototype.upcast(collisionObj);
    if (rigidBody) {
      const motionState = rigidBody.getMotionState();
      if (motionState) {
        try {
          this.Ammo.destroy(motionState);
        } catch (err) {
          console.error(`Error destroying motion state for rigid body on mesh ${meshName ?? '<Unknown>'}`);
          console.error(err);
        }
      }
    }

    // currently, every collision object owns its own shape, so we need to destroy it here
    //
    // we could re-use idential shape objects between collision objs in the future as an optimization, at which
    // point we would need to refcount them or something and not destroy them here.
    try {
      const collisionShape = collisionObj.getCollisionShape();
      if (collisionShape) {
        this.Ammo.destroy(collisionShape);
      }
    } catch (err) {
      console.error(`Error destroying collision shape for mesh ${meshName ?? '<Unknown>'}`, { collisionObj });
      console.error(err);
    }
    this.Ammo.destroy(collisionObj);
  };

  private buildTrimeshShape = (
    indices: Uint16Array | undefined,
    vertices: Float32Array,
    scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
  ) => {
    // TODO: update IDL and use native indexed triangle mesh
    const trimesh = new this.Ammo.btTriangleMesh();
    trimesh.preallocateIndices((indices ?? vertices).length);
    trimesh.preallocateVertices(vertices.length);

    const v0 = new this.Ammo.btVector3();
    const v1 = new this.Ammo.btVector3();
    const v2 = new this.Ammo.btVector3();

    for (let i = 0; i < (indices ?? vertices).length; i += 3) {
      const i0 = indices ? indices[i] * 3 : i * 3;
      const i1 = indices ? indices[i + 1] * 3 : i * 3 + 3;
      const i2 = indices ? indices[i + 2] * 3 : i * 3 + 6;
      v0.setValue(vertices[i0] * scale.x, vertices[i0 + 1] * scale.y, vertices[i0 + 2] * scale.z);
      v1.setValue(vertices[i1] * scale.x, vertices[i1 + 1] * scale.y, vertices[i1 + 2] * scale.z);
      v2.setValue(vertices[i2] * scale.x, vertices[i2 + 1] * scale.y, vertices[i2 + 2] * scale.z);

      // TODO: compute triangle area and log about ones that are too big or too small
      // Area of triangles should be <10 units, as suggested by user guide
      // Should be greater than 0.05 or something like that too probably
      trimesh.addTriangle(v0, v1, v2);
    }
    this.Ammo.destroy(v0);
    this.Ammo.destroy(v1);
    this.Ammo.destroy(v2);

    const shape = new this.Ammo.btBvhTriangleMeshShape(trimesh, true, true);
    return shape;
  };

  private buildCollisionShapeFromMesh = (mesh: THREE.Mesh, extraScale?: THREE.Vector3) => {
    if (mesh.geometry instanceof THREE.BoxGeometry) {
      const halfExtents = this.btvec3(
        mesh.geometry.parameters.width * mesh.scale.x * (extraScale?.x ?? 1) * 0.5,
        mesh.geometry.parameters.height * mesh.scale.y * (extraScale?.y ?? 1) * 0.5,
        mesh.geometry.parameters.depth * mesh.scale.z * (extraScale?.z ?? 1) * 0.5
      );
      return new this.Ammo.btBoxShape(halfExtents);
    } else if (
      (mesh.geometry instanceof THREE.SphereGeometry ||
        (mesh.geometry instanceof THREE.IcosahedronGeometry && mesh.geometry.parameters.detail >= 2)) &&
      mesh.scale.x === mesh.scale.y &&
      mesh.scale.y === mesh.scale.z &&
      (!extraScale || (extraScale.x === extraScale.y && extraScale.y === extraScale.z))
    ) {
      const radius = mesh.geometry.parameters.radius * mesh.scale.x * (extraScale?.x ?? 1);
      return new this.Ammo.btSphereShape(radius);
    }

    const geometry = mesh.geometry as THREE.BufferGeometry;
    const vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
    const indices = geometry.index?.array as Uint16Array | undefined;
    if (vertices instanceof Uint16Array) {
      throw new Error('GLTF Quantization not yet supported');
    }
    let scale = mesh.scale.clone();
    if (extraScale) {
      scale = scale.multiply(extraScale);
    }

    if (mesh.userData.convexhull || mesh.userData.convexHull) {
      return this.buildConvexHullShape(indices, vertices, scale);
    }
    return this.buildTrimeshShape(indices, vertices, scale);
  };

  public teleportPlayer = (
    pos: THREE.Vector3 | [number, number, number],
    rot?: THREE.Vector3 | [number, number, number]
  ) => {
    this.playerController.warp(
      Array.isArray(pos)
        ? this.btvec3(pos[0], pos[1] + this.playerColliderHeight / 2 + this.playerColliderRadius, pos[2])
        : this.btvec3(pos.x, pos.y + this.playerColliderHeight / 2 + this.playerColliderRadius, pos.z)
    );
    if (rot) {
      const r = Array.isArray(rot) ? rot : [rot.x, rot.y, rot.z];
      this.viz.cameraController?.setAngles(r[0] + Math.PI / 2, r[1]);
    }
    this.playerController.setExternalVelocity(this.btvec3(0, 0, 0));
    this.playerController.setVerticalVelocity(0);
    this.playerController.setOnGround(false);
  };

  public addTriMesh = (mesh: THREE.Mesh, colliderType: 'static' | 'kinematic' = 'static') => {
    if (mesh.userData.nocollide || mesh.name.includes('nocollide')) {
      return;
    }

    if (
      (mesh.material instanceof CustomShaderMaterial &&
        mesh.material.materialClass === MaterialClass.Instakill) ||
      mesh.userData.instakill
    ) {
      const collisionObj = this.addPlayerRegionContactCb({ type: 'mesh', mesh }, () =>
        this.viz.onInstakillTerrainCollision(collisionObj, mesh)
      );
      mesh.userData.collisionObj = collisionObj;
      return;
    }

    const shape = this.buildCollisionShapeFromMesh(mesh);
    const objRef: CollisionObjectRef = {
      materialClass: mesh.material instanceof CustomShaderMaterial ? mesh.material.materialClass : undefined,
    };
    const rigidBody = this.addCollisionObject(shape, mesh.position, mesh.quaternion, objRef, colliderType);
    mesh.userData.rigidBody = rigidBody;
  };

  public addPlayerRegionContactCb: AddPlayerRegionContactCB = (
    region,
    onEnter,
    onLeave,
    minPenetrationDepth = 0.04
  ) => {
    if (!onEnter && !onLeave) {
      throw new Error('Must provide at least one callback');
    }

    const ghostObj = this.createZoneGhostObject(region);
    const zoneId = this.nextZoneId++;
    const sensor = new this.Ammo.btSensor(ghostObj, zoneId, minPenetrationDepth);
    this.playerController.addSensor(sensor);

    this.zoneCallbacks.set(zoneId, { onEnter, onLeave });
    this.sensorEntries.push({ sensor, ghostObj, zoneId });

    return ghostObj;
  };

  public removePlayerRegionContactCb = (ghostObj: BtPairCachingGhostObject, destroyCollisionObj = true) => {
    const ix = this.sensorEntries.findIndex(e => e.ghostObj === ghostObj);
    if (ix === -1) {
      console.warn('No sensor registered for given collision object');
      return;
    }

    const entry = this.sensorEntries[ix];
    this.sensorEntries[ix] = this.sensorEntries[this.sensorEntries.length - 1];
    this.sensorEntries.pop();

    this.playerController.removeSensor(entry.sensor);
    this.zoneCallbacks.delete(entry.zoneId);
    this.collisionWorld.removeCollisionObject(ghostObj);
    this.Ammo.destroy(entry.sensor);
    if (destroyCollisionObj) {
      try {
        this.Ammo.destroy(ghostObj);
      } catch (err) {
        console.error('Error destroying ghostObj', ghostObj, err);
      }
    }
  };

  public addJumpPad = (
    region: ContactRegion,
    config: {
      baseImpulse: number;
      speedScaling: number;
      cooldownSeconds: number;
      direction: THREE.Vector3;
    },
    onTrigger?: () => void
  ): JumpPadEntry => {
    const ghostObj = this.createZoneGhostObject(region);
    const zoneId = this.nextZoneId++;
    const pad = new this.Ammo.btJumpPad(
      ghostObj,
      zoneId,
      config.baseImpulse,
      config.speedScaling,
      config.cooldownSeconds
    );
    pad.setDirection(this.btvec3(config.direction.x, config.direction.y, config.direction.z));
    this.playerController.addJumpPad(pad);

    this.zoneCallbacks.set(zoneId, { onEnter: onTrigger });
    const entry: JumpPadEntry = { pad, ghostObj, zoneId };
    this.jumpPads.push(entry);
    return entry;
  };

  public removeJumpPad = (entry: JumpPadEntry) => {
    const ix = this.jumpPads.indexOf(entry);
    if (ix === -1) {
      console.warn('Jump pad not registered');
      return;
    }
    this.jumpPads[ix] = this.jumpPads[this.jumpPads.length - 1];
    this.jumpPads.pop();

    this.playerController.removeJumpPad(entry.pad);
    this.zoneCallbacks.delete(entry.zoneId);
    this.collisionWorld.removeCollisionObject(entry.ghostObj);
    this.Ammo.destroy(entry.pad);
    this.Ammo.destroy(entry.ghostObj);
  };

  public addBoostZone = (
    region: ContactRegion,
    config: {
      strength: number;
      directionalBias: number;
      direction: THREE.Vector3;
    },
    onEnter?: () => void,
    onExit?: () => void
  ): BoostZoneEntry => {
    const ghostObj = this.createZoneGhostObject(region);
    const zoneId = this.nextZoneId++;
    const zone = new this.Ammo.btBoostZone(ghostObj, zoneId, config.strength, config.directionalBias);
    zone.setDirection(this.btvec3(config.direction.x, config.direction.y, config.direction.z));
    this.playerController.addBoostZone(zone);

    this.zoneCallbacks.set(zoneId, { onEnter, onLeave: onExit });
    const entry: BoostZoneEntry = { zone, ghostObj, zoneId };
    this.boostZones.push(entry);
    return entry;
  };

  public removeBoostZone = (entry: BoostZoneEntry) => {
    const ix = this.boostZones.indexOf(entry);
    if (ix === -1) {
      console.warn('Boost zone not registered');
      return;
    }
    this.boostZones[ix] = this.boostZones[this.boostZones.length - 1];
    this.boostZones.pop();

    this.playerController.removeBoostZone(entry.zone);
    this.zoneCallbacks.delete(entry.zoneId);
    this.collisionWorld.removeCollisionObject(entry.ghostObj);
    this.Ammo.destroy(entry.zone);
    this.Ammo.destroy(entry.ghostObj);
  };

  private createZoneGhostObject = (region: ContactRegion): BtPairCachingGhostObject => {
    const { shape, transform } = (() => {
      switch (region.type) {
        case 'box': {
          const shape = new this.Ammo.btBoxShape(
            this.btvec3(region.halfExtents.x, region.halfExtents.y, region.halfExtents.z)
          );
          const transform = new this.Ammo.btTransform();
          transform.setIdentity();
          transform.setOrigin(this.btvec3(region.pos.x, region.pos.y, region.pos.z));
          if (region.quat) {
            const rot = new this.Ammo.btQuaternion(
              region.quat.x,
              region.quat.y,
              region.quat.z,
              region.quat.w
            );
            transform.setRotation(rot);
            this.Ammo.destroy(rot);
          }
          return { shape, transform };
        }
        case 'mesh': {
          const { mesh } = region;
          return withWorldSpaceTransform(mesh, mesh => {
            // buildCollisionShapeFromMesh reads mesh.scale (now world-space) internally,
            // so only pass margin/region.scale as an additional multiplier.
            let extraScale: THREE.Vector3 | undefined;
            if (region.margin || region.scale) {
              extraScale = new THREE.Vector3(1, 1, 1).multiplyScalar(1 + (region.margin ?? 0));
              if (region.scale) {
                extraScale = extraScale.multiply(region.scale);
              }
            }
            const shape = this.buildCollisionShapeFromMesh(region.mesh, extraScale);
            const transform = new this.Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(this.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
            if (mesh.quaternion) {
              const rot = new this.Ammo.btQuaternion(
                mesh.quaternion.x,
                mesh.quaternion.y,
                mesh.quaternion.z,
                mesh.quaternion.w
              );
              transform.setRotation(rot);
              this.Ammo.destroy(rot);
            }
            return { shape, transform };
          });
        }
        case 'sphere': {
          const shape = new this.Ammo.btSphereShape(region.radius);
          const transform = new this.Ammo.btTransform();
          transform.setIdentity();
          transform.setOrigin(this.btvec3(region.pos.x, region.pos.y, region.pos.z));
          return { shape, transform };
        }
        default: {
          // For convexHull and aabb, fall through to mesh-based approach
          const mesh = (region as any).mesh as THREE.Mesh;
          const shape = this.buildCollisionShapeFromMesh(mesh);
          const transform = new this.Ammo.btTransform();
          transform.setIdentity();
          transform.setOrigin(this.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
          if (mesh.quaternion) {
            const rot = new this.Ammo.btQuaternion(
              mesh.quaternion.x,
              mesh.quaternion.y,
              mesh.quaternion.z,
              mesh.quaternion.w
            );
            transform.setRotation(rot);
            this.Ammo.destroy(rot);
          }
          return { shape, transform };
        }
      }
    })();

    const obj = new this.Ammo.btPairCachingGhostObject();
    obj.setWorldTransform(transform);
    obj.setCollisionShape(shape);
    obj.setCollisionFlags(4); // btCollisionObject::CF_NO_CONTACT_RESPONSE
    this.Ammo.destroy(transform);

    this.collisionWorld.addCollisionObject(
      obj,
      1, // btBroadphaseProxy::StaticFilter
      32 // btBroadphaseProxy::CharacterFilter
    );

    return obj;
  };

  public addBox = (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const shape = new this.Ammo.btBoxShape(this.btvec3(...halfExtents));
    this.addCollisionObject(shape, new THREE.Vector3(...pos), quat, undefined, colliderType);
  };

  public addCone = (
    pos: THREE.Vector3,
    radius: number,
    height: number,
    quat?: THREE.Quaternion,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const shape = new this.Ammo.btConeShape(radius, height);
    this.addCollisionObject(shape, pos, quat, undefined, colliderType);
  };

  public addCompound = (
    pos: [number, number, number],
    children: {
      type: 'box';
      pos: [number, number, number];
      halfExtents: [number, number, number];
      quat?: THREE.Quaternion;
    }[],
    quat?: THREE.Quaternion
  ) => {
    const parentShape = new this.Ammo.btCompoundShape(true);

    const childTransform = new this.Ammo.btTransform();
    for (const child of children) {
      const childShape = (() => {
        if (child.type === 'box') {
          return new this.Ammo.btBoxShape(this.btvec3(...child.halfExtents));
        }

        throw new Error('Unimplemented');
      })();

      childTransform.setIdentity();
      childTransform.setOrigin(this.btvec3(...child.pos));
      if (child.quat) {
        const rot = new this.Ammo.btQuaternion(child.quat.x, child.quat.y, child.quat.z, child.quat.w);
        childTransform.setRotation(rot);
        this.Ammo.destroy(rot);
      }
      parentShape.addChildShape(childTransform, childShape);
    }

    this.addCollisionObject(parentShape, new THREE.Vector3(...pos), quat);

    this.Ammo.destroy(childTransform);
  };

  public optimize = () => this.broadphase.optimize();

  private clearCollisionWorld = () => {
    while (this.jumpPads.length > 0) {
      this.removeJumpPad(this.jumpPads[0]);
    }
    while (this.boostZones.length > 0) {
      this.removeBoostZone(this.boostZones[0]);
    }
    while (this.sensorEntries.length > 0) {
      this.removePlayerRegionContactCb(this.sensorEntries[0].ghostObj);
    }
    this.zoneCallbacks.clear();

    // TODO: Probably need to actually destroy everything in the old world...
    const newCollisionWorld = new this.Ammo.btDiscreteDynamicsWorld(
      this.dispatcher,
      this.broadphase,
      this.solver,
      this.collisionConfiguration
    );
    this.Ammo.destroy(this.collisionWorld);
    this.collisionWorld = newCollisionWorld;
  };

  public destroy = () => {
    this.releaseReplayPlayer();
    this.flightRecorder.destroy();
    for (const entry of this.physicsTickerEntries) {
      if (entry.lastBodyTransform) {
        this.Ammo.destroy(entry.lastBodyTransform);
      }
    }
    this.physicsTickerEntries = [];
    if (this.scratchQuat) {
      this.Ammo.destroy(this.scratchQuat);
      this.scratchQuat = null;
    }

    this.clearCollisionWorld();

    this.Ammo.destroy(this.broadphase);
    this.Ammo.destroy(this.collisionConfiguration);
    this.Ammo.destroy(this.dispatcher);
    this.Ammo.destroy(this.solver);
    this.Ammo.destroy(this.playerController);
    this.Ammo.destroy(this.playerGhostObject);
    this.Ammo.destroy(this.collisionWorld);
  };

  private buildConvexHullShape = (
    indices: Uint16Array | undefined,
    vertices: Float32Array,
    scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
  ) => {
    const hull = new this.Ammo.btConvexHullShape();

    if (indices) {
      for (let i = 0; i < indices.length; i++) {
        const ix = indices[i] * 3;
        hull.addPoint(
          this.btvec3(vertices[ix] * scale.x, vertices[ix + 1] * scale.y, vertices[ix + 2] * scale.z)
        );
      }
    } else {
      for (let i = 0; i < vertices.length; i += 3) {
        hull.addPoint(
          this.btvec3(vertices[i] * scale.x, vertices[i + 1] * scale.y, vertices[i + 2] * scale.z)
        );
      }
    }

    return hull;
  };

  public addHeightmapTerrain = (
    heightmapData: Float32Array,
    minHeight: number,
    maxHeight: number,
    gridResolutionX: number,
    gridResolutionY: number,
    worldSpaceWidth: number,
    worldSpaceLength: number
  ) => {
    // heightScale: only matters if using non-float heightmap data; multiplied by values in `heightmapData`
    //              to get the actual height
    // heightStickWidth: x dimension of `heightmapData` array
    // heightStickLength: y dimension of `heightmapData` array
    // heightfieldData: 2D array of heights
    //
    // I'm pretty sure that scaling the heightmap in XZ space is done via `localScaling` ...
    // I can't imagine how else it would be.
    //
    // btHeightfieldTerrainShape::btHeightfieldTerrainShape(
    //   int heightStickWidth, int heightStickLength, const void* heightfieldData,
    //   btScalar heightScale, btScalar minHeight, btScalar maxHeight, int upAxis,
    //   PHY_ScalarType hdt, bool flipQuadEdges){}
    const terrainDataPtr = this.Ammo._malloc(4 * heightmapData.length);
    const terrainData = new Float32Array(this.Ammo.HEAPF32.buffer, terrainDataPtr, heightmapData.length);
    terrainData.set(heightmapData);

    // The center between minHeight and maxHeight must be zero, otherwise the terrain will be
    // offset due to the way the heightfield collision is implemented in bullet.
    const oldCenter = (minHeight + maxHeight) / 2;
    // we can only expand the range; the new minHeight must be <= the old minHeight and
    // the new maxHeight must be >= the old maxHeight
    const newCenter = 0;
    const deltaCenter = newCenter - oldCenter;
    if (deltaCenter < 0) {
      minHeight += deltaCenter;
    } else {
      maxHeight += deltaCenter;
    }

    const heightfieldShape = new this.Ammo.btHeightfieldTerrainShape(
      gridResolutionX,
      gridResolutionY,
      terrainDataPtr,
      1, // heightScale
      minHeight,
      maxHeight,
      1, // upAxis
      false // flipQuadEdges
    );

    heightfieldShape.setLocalScaling(
      this.btvec3(worldSpaceWidth / (gridResolutionX - 1), 1, worldSpaceLength / (gridResolutionY - 1))
    );

    this.addCollisionObject(heightfieldShape, new THREE.Vector3(0, 0, 0));
  };

  public registerDashCb = (cb: (curTimeSeconds: number) => void) => this.dashManager.registerDashCb(cb);
  public deregisterDashCb = (cb: (curTimeSeconds: number) => void) => this.dashManager.deregisterDashCb(cb);
}
