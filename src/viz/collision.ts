import * as THREE from 'three';
import { derived, type Readable } from 'svelte/store';

import type { FpPlayerStateGetters, Viz } from './index.js';
import {
  DefaultExternalVelocityAirDampingFactor,
  DefaultExternalVelocityGroundDampingFactor,
  DefaultMoveSpeed,
  DefaultOOBThreshold,
  DefaultTopDownCameraOffset,
  type SceneConfig,
} from './scenes/index.js';
import { CustomShaderMaterial } from './shaders/customShader';
import { MaterialClass } from './shaders/customShader.js';
import {
  DefaultGravity,
  DefaultJumpSpeed,
  DefaultPlayerColliderHeight,
  DefaultPlayerColliderRadius,
  DefaultPlayerColliderShape,
} from './conf.js';
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
import { clamp } from './util/util.js';

let ammojs: Promise<AmmoInterface> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) {
    return ammojs;
  }

  ammojs = import('../ammojs/ammo.wasm.js').then(mod => (mod as any).Ammo.apply({}));
  return ammojs;
};

export type ContactRegion =
  | { type: 'box'; pos: THREE.Vector3; halfExtents: THREE.Vector3; quat?: THREE.Quaternion }
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
const MAX_SLOPE_RADS = 0.8;
// \/ This is a very important config item for the physics engine.  Setting it too high will result in
// the player vibrating and janking out when pushing into corners and similar.  Setting too low causes
// weird issues where the player slides around on the floor or clips through geometry.
const MAX_PENETRATION_DEPTH = 0.075;
const DEFAULT_SIMULATION_TICK_RATE_HZ = 160;
const MIN_JUMP_DELAY_SECONDS = 0.25; // TODO: make configurable

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
  /**
   * If easy mode is true, then magnitude is normalized to what it would be if the user was moving
   * diagonally, allowing for easier movement.
   */
  public easyModeMovement: Readable<boolean>;

  private jumpCbs: ((curTimeSeconds: number) => void)[] = [];
  private lastJumpTimeSeconds = 0;
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
  private physicsTickerEntries: PhysicsTickerEntry[] = [];
  private scratchQuat: BtQuaternion | null = null;

  constructor({ viz, Ammo, initialSpawnPos }: BulletPhysicsArgs) {
    this.Ammo = Ammo;
    this.viz = viz;
    this.simulationTickRate = viz.sceneConf.simulationTickRate ?? DEFAULT_SIMULATION_TICK_RATE_HZ;

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

    this.installMouseInputHandlers();

    this.initGlobalConsoleHelpers();

    this.dashManager = new DashManager(viz);

    if (localStorage.goBackOnLoad && localStorage.backPos) {
      (window as any).back();
      delete localStorage.goBackOnLoad;
    } else {
      this.teleportPlayer(viz.spawnPos.pos, viz.spawnPos.rot);
    }

    this.easyModeMovement = derived(viz.vizConfig, vizConfig => vizConfig.gameplay.easyModeMovement);

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
        this.playerController.isJumping() && this.lastJumpTimeSeconds > this.dashManager.lastDashTimeSeconds,
      getIsDashing: () =>
        this.playerController.isJumping() && this.dashManager.lastDashTimeSeconds > this.lastJumpTimeSeconds,
    };

    this.startMainGameTick();
  }

  private get gravity() {
    return this.viz.sceneConf.gravity ?? 40;
  }
  private get jumpSpeed() {
    return this.viz.sceneConf.player?.jumpVelocity ?? 20;
  }
  private get playerColliderHeight() {
    return this.viz.sceneConf.player?.colliderSize?.height ?? DefaultPlayerColliderHeight;
  }
  private get playerColliderRadius() {
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
        initialSpawnPos.pos.y + this.playerColliderHeight,
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
    this.playerController.setMaxPenetrationDepth(MAX_PENETRATION_DEPTH);
    this.playerController.setMaxSlope(MAX_SLOPE_RADS);
    this.playerController.setStepHeight(playerStepHeight);
    this.playerController.setJumpSpeed(this.viz.sceneConf.player?.jumpVelocity ?? DefaultJumpSpeed);

    this.collisionWorld.addCollisionObject(
      this.playerGhostObject,
      32, // btBroadphaseProxy::CharacterFilter
      1 | 2 // btBroadphaseProxy::StaticFilter | btBroadphaseProxy::DefaultFilter
    );
    this.collisionWorld.addAction(this.playerController);

    this.setGravity(this.viz.sceneConf.gravity ?? DefaultGravity);

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
  };

  private installMouseInputHandlers = () => {
    const handlerID = Math.random();
    if (window.location?.href.includes('localhost')) {
      document.body.addEventListener('mousedown', evt => {
        if (evt.button === 3) {
          (window as any).back();
        }
      });
    }

    const cameraEulerScratch = new THREE.Euler();
    document.body.addEventListener('mousemove', evt => {
      if (
        document.pointerLockElement !== document.body ||
        this.viz.sceneConf.viewMode!.type !== 'firstPerson' ||
        !this.viz.controlState.cameraControlEnabled
      ) {
        return;
      }

      cameraEulerScratch.setFromQuaternion(this.viz.camera.quaternion, 'YXZ');

      const mouseSensitivity = this.viz.vizConfig.current.controls.mouseSensitivity;
      // sometimes some freak shit happens where large mouse movements get reported twice ... ...
      // if (Math.abs(evt.movementX) > 100 || Math.abs(evt.movementY) > 100) {
      //   console.warn(evt.movementX, evt.movementY, handlerID);
      // }
      cameraEulerScratch.y -= evt.movementX * mouseSensitivity * 0.001;
      cameraEulerScratch.x -= evt.movementY * mouseSensitivity * 0.001;

      // Clamp the camera's rotation to the range of -PI/2 to PI/2
      // This is so the camera doesn't flip upside down
      cameraEulerScratch.x = clamp(cameraEulerScratch.x, -Math.PI / 2 + 0.1, Math.PI / 2 - 0.001);

      this.viz.camera.quaternion.setFromEuler(cameraEulerScratch);
    });
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

  private startMainGameTick = () => {
    const teleportPlayerIfOOB = () => {
      if (this.viz.camera.position.y <= (this.viz.sceneConf.player?.oobYThreshold ?? DefaultOOBThreshold)) {
        this.viz.respawnPlayer();
      }
    };

    this.viz.registerBeforeRenderCb(
      (curTimeSecs, tDiffSecs) => {
        const newPlayerPos = this.updateCollisionWorld(curTimeSecs, tDiffSecs);
        if (this.viz.sceneConf.player?.mesh) {
          this.viz.sceneConf.player.mesh.position.copy(newPlayerPos);
        }

        if (this.viz.controlState.cameraControlEnabled) {
          const cameraPos = this.computeCameraPos(newPlayerPos, this.viz.sceneConf.viewMode! as any);
          this.viz.camera.position.copy(cameraPos);
        }

        teleportPlayerIfOOB();
      },
      // Setting this priority ensures that the physics simulation always runs last, after all user-supplied
      // callbacks have been called.  This avoids issues where the visual positions of objects that are
      // animated by the user don't line up with the collision world positions.
      Infinity
    );
  };

  public computeCameraPos = (
    newPlayerPos: THREE.Vector3,
    viewMode: Extract<NonNullable<SceneConfig['viewMode']>, { type: 'firstPerson' | 'top-down' }>
  ) => {
    newPlayerPos.y += 0.5 * this.playerColliderHeight;
    switch (viewMode.type) {
      case 'firstPerson':
        return newPlayerPos;
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

  private handleFirstPersonInput = () => {
    this.forwardDir = this.viz.camera.getWorldDirection(this.forwardDir).normalize();
    this.leftDir = this.leftDir.crossVectors(this.upDir, this.forwardDir).normalize();
    // Adjust `forwardDir` to be horizontal.
    this.forwardDir = this.forwardDir.crossVectors(this.leftDir, this.upDir).normalize();

    if (this.viz.keyStates['KeyW']) this.moveDirection.add(this.forwardDir);
    if (this.viz.keyStates['KeyS']) this.moveDirection.sub(this.forwardDir);
    if (this.viz.keyStates['KeyA']) this.moveDirection.add(this.leftDir);
    if (this.viz.keyStates['KeyD']) this.moveDirection.sub(this.leftDir);
  };

  private handleTopDownInput = () => {
    // camera looks towards negative Y.  negative X is left, negative Z is down
    if (this.viz.keyStates['KeyW']) this.moveDirection.add(new THREE.Vector3(0, 0, 1));
    if (this.viz.keyStates['KeyS']) this.moveDirection.add(new THREE.Vector3(0, 0, -1));
    if (this.viz.keyStates['KeyA']) this.moveDirection.add(new THREE.Vector3(1, 0, 0));
    if (this.viz.keyStates['KeyD']) this.moveDirection.add(new THREE.Vector3(-1, 0, 0));
  };

  private handleInput = (curTimeSeconds: number) => {
    const cameraDir = this.viz.camera.getWorldDirection(this.forwardDir).normalize().clone();
    const wasOnGround = this.playerController.onGround();

    this.moveDirection.set(0, 0, 0);
    if (this.viz.controlState.movementEnabled) {
      const viewMode = this.getViewMode();
      switch (viewMode.type) {
        case 'firstPerson':
          this.handleFirstPersonInput();
          break;
        case 'top-down':
          this.handleTopDownInput();
          break;
        default:
          viewMode satisfies never;
          throw new Error(`Unknown view mode: ${viewMode}`);
      }
    }

    if (this.viz.vizConfig.current.gameplay.easyModeMovement) {
      const targetMagnitude = new THREE.Vector3().add(this.forwardDir).add(this.leftDir).length();
      const magnitude = this.moveDirection.length();
      if (magnitude > 0) {
        this.moveDirection.multiplyScalar(targetMagnitude / magnitude);
      }
    }

    if (this.viz.controlState.movementEnabled && this.viz.keyStates['Space'] && wasOnGround) {
      if (curTimeSeconds - this.lastJumpTimeSeconds > MIN_JUMP_DELAY_SECONDS) {
        this.playerController.jump(
          this.btvec3(
            this.moveDirection.x * (this.jumpSpeed * 0.18),
            this.jumpSpeed,
            this.moveDirection.z * (this.jumpSpeed * 0.18)
          )
        );
        // playerController.setExternalVelocity(
        //   this.btvec3(
        //     moveDirection.x * (jumpSpeed * 0.18) * 0.01,
        //     jumpSpeed * 0.01,
        //     moveDirection.z * (jumpSpeed * 0.18) * 0.01
        //   )
        // );
        this.lastJumpTimeSeconds = curTimeSeconds;
        for (const cb of this.jumpCbs) {
          cb(curTimeSeconds);
        }
      }
    }

    const wasWalking = this.isWalking;
    this.isWalking = this.moveDirection.x !== 0 || this.moveDirection.y !== 0 || this.moveDirection.z !== 0;
    if (wasWalking && !this.isWalking) {
      this.viz.sfxManager.onWalkStop();
    } else if (!wasWalking && this.isWalking) {
      this.viz.sfxManager.onWalkStart(MaterialClass.Default);
    }

    this.dashManager.tick(curTimeSeconds, wasOnGround);

    if (this.viz.keyStates['ShiftLeft'] || this.viz.keyStates['ShiftRight']) {
      const dashDir = this.getDashDir(this.getViewMode().type, cameraDir);
      this.dashManager.tryDash(curTimeSeconds, this.isFlyMode, dashDir);
    }
  };

  private getDashDir = (viewModeType: 'firstPerson' | 'top-down', cameraDir: THREE.Vector3) => {
    switch (viewModeType) {
      case 'firstPerson':
        return cameraDir;
      case 'top-down':
        return this.moveDirection.clone().lerp(this.upDir, 0.5).normalize();
      default:
        viewModeType satisfies never;
        throw new Error(`Unknown view mode: ${viewModeType}`);
    }
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
    { type: 'firstPerson' | 'top-down' }
  > => {
    const viewMode = this.viz.sceneConf.viewMode!;
    if (viewMode.type !== 'firstPerson' && viewMode.type !== 'top-down') {
      throw new Error(
        `View mode must be 'firstPerson' or 'top-down' for collision; found: '${viewMode.type}'`
      );
    }

    return viewMode;
  };

  public registerJumpCb = (cb: (curTimeSeconds: number) => void) => {
    this.jumpCbs.push(cb);
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
        case ZoneEventType.SensorEnter:
        case ZoneEventType.JumpPadTriggered:
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

  /**
   * Returns the new position of the player.
   */
  public updateCollisionWorld = (curTimeSeconds: number, tDiffSeconds: number): THREE.Vector3 => {
    const wasOnGround = this.playerController.onGround();

    this.handleInput(curTimeSeconds);

    const playerMoveSpeed = this.viz.sceneConf.player?.moveSpeed ?? DefaultMoveSpeed;
    const moveSpeedPerSecond = wasOnGround ? playerMoveSpeed.onGround : playerMoveSpeed.inAir;
    const walkDirBulletVector = this.btvec3(
      this.moveDirection.x * moveSpeedPerSecond,
      this.moveDirection.y * moveSpeedPerSecond,
      this.moveDirection.z * moveSpeedPerSecond
    );
    this.playerController.setWalkDirection(walkDirBulletVector);
    this.playerController.resetForcedRotation();

    const fixedTimeStep = 1 / this.simulationTickRate;
    const maxSubSteps = 20;

    const numSubSteps = this.collisionWorld.beginStepSimulation(tDiffSeconds, maxSubSteps, fixedTimeStep);
    for (let i = 0; i < numSubSteps; i++) {
      this.physicsElapsedTime += fixedTimeStep;
      this.tickPhysicsTickers(fixedTimeStep);
      this.collisionWorld.substepSimulation();
      this.drainZoneEvents();
    }
    this.collisionWorld.finishStepSimulation();
    this.syncPhysicsTickerVisuals();

    const newPlayerTransform = this.playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();

    const nowOnGround = this.playerController.onGround();
    if (!wasOnGround && nowOnGround) {
      const landedOnObjectIx: number = this.playerController.getFloorUserIndex();
      const landedOnObject = this.collisionObjectRefs.get(landedOnObjectIx);
      const materialClass = landedOnObject?.materialClass ?? MaterialClass.Default;
      this.viz.sfxManager.onPlayerLand(materialClass);
    }

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  public reset = () => {
    for (const key of Object.keys(this.viz.keyStates)) {
      this.viz.keyStates[key] = false;
    }
    this.playerController.setExternalVelocity(this.btvec3(0, 0, 0));
    this.playerController.setVerticalVelocity(0);
    this.playerController.setOnGround(false);
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
        ? this.btvec3(pos[0], pos[1] + this.playerColliderHeight, pos[2])
        : this.btvec3(pos.x, pos.y + this.playerColliderHeight, pos.z)
    );
    if (rot && this.getViewMode().type === 'firstPerson') {
      this.viz.camera.rotation.setFromVector3(
        Array.isArray(rot) ? new THREE.Vector3(rot[0], rot[1], rot[2]) : rot
      );
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
    config: { baseImpulse: number; speedScaling: number; cooldownSeconds: number; direction: THREE.Vector3 },
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
    config: { strength: number; directionalBias: number; direction: THREE.Vector3 },
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
          let scale = mesh.scale.clone().multiplyScalar(1 + (region.margin ?? 0));
          if (region.scale) {
            scale = scale.multiply(region.scale);
          }
          const shape = this.buildCollisionShapeFromMesh(region.mesh, scale);
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
