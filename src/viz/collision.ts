import { derived } from 'svelte/store';
import * as THREE from 'three';

import type { FpPlayerStateGetters, Viz } from './index.js';
import {
  type DashConfig,
  DefaultDashConfig,
  DefaultExternalVelocityAirDampingFactor,
  DefaultExternalVelocityGroundDampingFactor,
  DefaultMoveSpeed,
  type SceneConfig,
} from './scenes/index.js';
import { CustomShaderMaterial } from './shaders/customShader';
import { MaterialClass } from './shaders/customShader.js';
import { DefaultPlayerColliderHeight, DefaultPlayerColliderRadius } from './conf.js';
import type {
  AmmoInterface,
  BtCollisionObject,
  BtCollisionShape,
  BtDiscreteDynamicsWorld,
  BtKinematicCharacterController,
  BtPairCachingGhostObject,
  BtRigidBody,
  BtVec3,
} from '../ammojs/ammoTypes';
import { DashManager } from './DashManager.js';

let ammojs: Promise<AmmoInterface> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) return ammojs;
  ammojs = import('../ammojs/ammo.wasm.js').then(mod => (mod as any).Ammo.apply({}));
  return ammojs;
};

let btvec3: (x: number, y: number, z: number) => BtVec3 = () => {
  throw new Error('btvec3 not initialized');
};

const initBtvec3Scratch = (Ammo: AmmoInterface) => {
  const scratchVec: BtVec3 = new Ammo.btVector3();
  btvec3 = (x: number, y: number, z: number) => {
    scratchVec.setValue(x, y, z);
    return scratchVec;
  };
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

interface BulletPhysicsArgs {
  viz: Viz;
  Ammo: AmmoInterface;
  gravity: number;
  jumpSpeed: number;
  playerColliderShape: 'capsule' | 'cylinder' | 'sphere';
  dashConfig: Partial<DashConfig> | undefined;
  externalVelocityAirDampingFactor: THREE.Vector3 | undefined;
  externalVelocityGroundDampingFactor: THREE.Vector3 | undefined;
  initialSpawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 };
  simulationTickRate: number | undefined;
}

export const initBulletPhysics = ({
  viz,
  Ammo,
  gravity,
  jumpSpeed,
  playerColliderShape,
  dashConfig = DefaultDashConfig,
  externalVelocityAirDampingFactor = DefaultExternalVelocityAirDampingFactor,
  externalVelocityGroundDampingFactor = DefaultExternalVelocityGroundDampingFactor,
  initialSpawnPos,
  simulationTickRate = DEFAULT_SIMULATION_TICK_RATE_HZ,
}: BulletPhysicsArgs) => {
  const collisionConfiguration = new Ammo.btDefaultCollisionConfiguration();
  const dispatcher = new Ammo.btCollisionDispatcher(collisionConfiguration);
  const broadphase = new Ammo.btDbvtBroadphase();
  const solver = new Ammo.btSequentialImpulseConstraintSolver();
  let collisionWorld: BtDiscreteDynamicsWorld = new Ammo.btDiscreteDynamicsWorld(
    dispatcher,
    broadphase,
    solver,
    collisionConfiguration
  );

  initBtvec3Scratch(Ammo);

  const playerColliderHeight = viz.sceneConf.player?.colliderSize?.height ?? DefaultPlayerColliderHeight;
  const playerColliderRadius = viz.sceneConf.player?.colliderSize?.radius ?? DefaultPlayerColliderRadius;

  const playerInitialTransform = new Ammo.btTransform();
  playerInitialTransform.setIdentity();
  playerInitialTransform.setOrigin(
    btvec3(initialSpawnPos.pos.x, initialSpawnPos.pos.y + playerColliderHeight, initialSpawnPos.pos.z)
  );
  const playerGhostObject = new Ammo.btPairCachingGhostObject();
  playerGhostObject.setWorldTransform(playerInitialTransform);
  Ammo.destroy(playerInitialTransform);
  collisionWorld
    .getBroadphase()
    .getOverlappingPairCache()
    .setInternalGhostPairCallback(new Ammo.btGhostPairCallback());
  const playerShape = ((): BtCollisionShape => {
    switch (playerColliderShape) {
      case 'capsule':
        return new Ammo.btCapsuleShape(playerColliderRadius, playerColliderHeight);
      case 'cylinder':
        const halfExtents = btvec3(playerColliderRadius, playerColliderHeight / 2, playerColliderRadius);
        return new Ammo.btCylinderShape(halfExtents);
      case 'sphere':
        return new Ammo.btSphereShape(playerColliderRadius);
      default:
        playerColliderShape satisfies never;
        throw new Error(
          `Unknown player collider shape: ${playerColliderShape}. Expected 'capsule' or 'cylinder'.`
        );
    }
  })();
  playerGhostObject.setCollisionShape(playerShape);
  playerGhostObject.setCollisionFlags(16); // btCollisionObject::CF_CHARACTER_OBJECT

  const playerStepHeight = viz.sceneConf.player?.stepHeight ?? DEFAULT_STEP_HEIGHT;
  const playerController: BtKinematicCharacterController = new Ammo.btKinematicCharacterController(
    playerGhostObject,
    playerShape,
    playerStepHeight,
    btvec3(0, 1, 0)
  );
  playerController.setMaxPenetrationDepth(MAX_PENETRATION_DEPTH);
  playerController.setMaxSlope(MAX_SLOPE_RADS);
  playerController.setStepHeight(playerStepHeight);
  playerController.setJumpSpeed(jumpSpeed);

  collisionWorld.addCollisionObject(
    playerGhostObject,
    32, // btBroadphaseProxy::CharacterFilter
    1 | 2 // btBroadphaseProxy::StaticFilter | btBroadphaseProxy::DefaultFilter
  );
  collisionWorld.addAction(playerController);

  const setGravity = (gravity: number) => {
    collisionWorld.setGravity(btvec3(0, -gravity, 0));
    playerController.setGravity(btvec3(0, -gravity, 0));
  };
  setGravity(gravity);

  playerController.setExternalVelocityAirDampingFactor(
    btvec3(
      externalVelocityAirDampingFactor.x,
      externalVelocityAirDampingFactor.y,
      externalVelocityAirDampingFactor.z
    )
  );
  playerController.setExternalVelocityGroundDampingFactor(
    btvec3(
      externalVelocityGroundDampingFactor.x,
      externalVelocityGroundDampingFactor.y,
      externalVelocityGroundDampingFactor.z
    )
  );

  /**
   * If easy mode is true, then magnitude is normalized to what it would be if the user was moving
   * diagonally, allowing for easier movement.
   */
  const easyModeMovement = derived(viz.vizConfig, vizConfig => vizConfig.gameplay.easyModeMovement);

  const jumpCbs: ((curTimeSeconds: number) => void)[] = [];
  const registerJumpCb = (cb: (curTimeSeconds: number) => void) => {
    jumpCbs.push(cb);
  };
  const deregisterJumpCb = (cb: (curTimeSeconds: number) => void) => {
    const ix = jumpCbs.indexOf(cb);
    if (ix === -1) {
      throw new Error('cb not registered');
    }
    jumpCbs.splice(ix, 1);
  };

  let lastJumpTimeSeconds = 0;
  const MIN_JUMP_DELAY_SECONDS = 0.25; // TODO: make configurable

  const dashManager = new DashManager(viz.sfxManager, dashConfig, playerController, btvec3);

  let isFlyMode = false;
  const setFlyMode = (newIsFlyMode?: boolean) => {
    if (newIsFlyMode === isFlyMode) {
      return;
    }

    isFlyMode = newIsFlyMode ?? !isFlyMode;
    setGravity(isFlyMode ? 0 : gravity);
  };

  const getViewMode = (): Extract<
    NonNullable<SceneConfig['viewMode']>,
    { type: 'firstPerson' | 'top-down' }
  > => {
    const viewMode = viz.sceneConf.viewMode!;
    if (viewMode.type !== 'firstPerson' && viewMode.type !== 'top-down') {
      throw new Error(
        `View mode must be 'firstPerson' or 'top-down' for collision; found: '${viewMode.type}'`
      );
    }

    return viewMode;
  };

  const moveDirection = new THREE.Vector3();
  let isWalking = false;
  const upDir = new THREE.Vector3(0, 1, 0);
  let forwardDir = new THREE.Vector3();
  let leftDir = new THREE.Vector3();

  const handleFirstPersonInput = () => {
    forwardDir = viz.camera.getWorldDirection(forwardDir).normalize();
    leftDir = leftDir.crossVectors(upDir, forwardDir).normalize();
    // Adjust `forwardDir` to be horizontal.
    forwardDir = forwardDir.crossVectors(leftDir, upDir).normalize();

    moveDirection.set(0, 0, 0);
    if (viz.keyStates['KeyW']) moveDirection.add(forwardDir);
    if (viz.keyStates['KeyS']) moveDirection.sub(forwardDir);
    if (viz.keyStates['KeyA']) moveDirection.add(leftDir);
    if (viz.keyStates['KeyD']) moveDirection.sub(leftDir);
  };

  const handleTopDownInput = () => {
    moveDirection.set(0, 0, 0);
    // camera looks towards negative Y.  negative X is left, negative Z is down
    if (viz.keyStates['KeyW']) moveDirection.add(new THREE.Vector3(0, 0, 1));
    if (viz.keyStates['KeyS']) moveDirection.add(new THREE.Vector3(0, 0, -1));
    if (viz.keyStates['KeyA']) moveDirection.add(new THREE.Vector3(1, 0, 0));
    if (viz.keyStates['KeyD']) moveDirection.add(new THREE.Vector3(-1, 0, 0));
  };

  const handleInput = (curTimeSeconds: number) => {
    const cameraDir = viz.camera.getWorldDirection(forwardDir).normalize().clone();
    const wasOnGround = playerController.onGround();

    if (viz.controlState.movementEnabled) {
      const viewMode = getViewMode();
      switch (viewMode.type) {
        case 'firstPerson':
          handleFirstPersonInput();
          break;
        case 'top-down':
          handleTopDownInput();
          break;
        default:
          viewMode satisfies never;
          throw new Error(`Unknown view mode: ${viewMode}`);
      }
    }

    if (viz.vizConfig.current.gameplay.easyModeMovement) {
      const targetMagnitude = new THREE.Vector3().add(forwardDir).add(leftDir).length();
      const magnitude = moveDirection.length();
      if (magnitude > 0) {
        moveDirection.multiplyScalar(targetMagnitude / magnitude);
      }
    }

    if (viz.keyStates['Space'] && wasOnGround) {
      if (curTimeSeconds - lastJumpTimeSeconds > MIN_JUMP_DELAY_SECONDS) {
        playerController.jump(
          btvec3(moveDirection.x * (jumpSpeed * 0.18), jumpSpeed, moveDirection.z * (jumpSpeed * 0.18))
        );
        // playerController.setExternalVelocity(
        //   btvec3(
        //     moveDirection.x * (jumpSpeed * 0.18) * 0.01,
        //     jumpSpeed * 0.01,
        //     moveDirection.z * (jumpSpeed * 0.18) * 0.01
        //   )
        // );
        lastJumpTimeSeconds = curTimeSeconds;
        for (const cb of jumpCbs) {
          cb(curTimeSeconds);
        }
      }
    }

    const wasWalking = isWalking;
    isWalking = moveDirection.x !== 0 || moveDirection.y !== 0 || moveDirection.z !== 0;
    if (wasWalking && !isWalking) {
      viz.sfxManager.onWalkStop();
    } else if (!wasWalking && isWalking) {
      viz.sfxManager.onWalkStart(MaterialClass.Default);
    }

    dashManager.tick(curTimeSeconds, wasOnGround);

    if (viz.keyStates['ShiftLeft'] || viz.keyStates['ShiftRight']) {
      const dashDir = getDashDir(getViewMode().type, cameraDir);
      dashManager.tryDash(curTimeSeconds, isFlyMode, dashDir);
    }
  };

  const getDashDir = (viewModeType: 'firstPerson' | 'top-down', cameraDir: THREE.Vector3) => {
    switch (viewModeType) {
      case 'firstPerson':
        return cameraDir;
      case 'top-down':
        return moveDirection.clone().lerp(upDir, 0.5).normalize();
      default:
        viewModeType satisfies never;
        throw new Error(`Unknown view mode: ${viewModeType}`);
    }
  };

  const tickCallbacks: ((tDiffSeconds: number) => void)[] = [];

  /**
   * Returns the new position of the player.
   */
  const updateCollisionWorld = (curTimeSeconds: number, tDiffSeconds: number): THREE.Vector3 => {
    const wasOnGround = playerController.onGround();

    handleInput(curTimeSeconds);

    const playerMoveSpeed = viz.sceneConf.player?.moveSpeed ?? DefaultMoveSpeed;
    const moveSpeedPerSecond = wasOnGround ? playerMoveSpeed.onGround : playerMoveSpeed.inAir;
    const walkDirBulletVector = btvec3(
      moveDirection.x * moveSpeedPerSecond,
      moveDirection.y * moveSpeedPerSecond,
      moveDirection.z * moveSpeedPerSecond
    );
    playerController.setWalkDirection(walkDirBulletVector);

    const fixedTimeStep = 1 / simulationTickRate;
    const maxSubSteps = 20;
    collisionWorld.stepSimulation(tDiffSeconds, maxSubSteps, fixedTimeStep);

    const newPlayerTransform = playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();

    const nowOnGround = playerController.onGround();
    if (!wasOnGround && nowOnGround) {
      const landedOnObjectIx: number = playerController.getFloorUserIndex();
      const landedOnObject = CollisionObjectRefs.get(landedOnObjectIx);
      const materialClass = landedOnObject?.materialClass ?? MaterialClass.Default;
      viz.sfxManager.onPlayerLand(materialClass);
    }

    for (const cb of tickCallbacks) {
      cb(tDiffSeconds);
    }

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  const teleportPlayer = (
    pos: THREE.Vector3 | [number, number, number],
    rot?: THREE.Vector3 | [number, number, number]
  ) => {
    playerController.warp(
      Array.isArray(pos)
        ? btvec3(pos[0], pos[1] + playerColliderHeight, pos[2])
        : btvec3(pos.x, pos.y + playerColliderHeight, pos.z)
    );
    if (rot && getViewMode().type === 'firstPerson') {
      viz.camera.rotation.setFromVector3(
        Array.isArray(rot) ? new THREE.Vector3(rot[0], rot[1], rot[2]) : rot
      );
    }
    playerController.setExternalVelocity(btvec3(0, 0, 0));
    playerController.setVerticalVelocity(0);
  };

  const reset = () => {
    for (const key of Object.keys(viz.keyStates)) {
      viz.keyStates[key] = false;
    }
    playerController.setExternalVelocity(btvec3(0, 0, 0));
    playerController.setVerticalVelocity(0);
  };

  teleportPlayer(viz.spawnPos.pos, viz.spawnPos.rot);

  interface CollisionObjectRef {
    materialClass?: MaterialClass;
  }

  let nextCollisionObjectRefId = 0;
  const CollisionObjectRefs: Map<number, CollisionObjectRef> = new Map();

  const addCollisionObject = (
    shape: BtCollisionShape,
    pos: THREE.Vector3,
    quat: THREE.Quaternion = new THREE.Quaternion(),
    objRef?: CollisionObjectRef,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const transform = new Ammo.btTransform();
    transform.setIdentity();
    transform.setOrigin(btvec3(pos.x, pos.y, pos.z));
    const rot = new Ammo.btQuaternion(quat.x, quat.y, quat.z, quat.w);
    transform.setRotation(rot);
    Ammo.destroy(rot);

    // Add the object as static, so it doesn't move but still collides
    const motionState = new Ammo.btDefaultMotionState(transform);
    const localInertia = btvec3(0, 0, 0);
    const rbInfo = new Ammo.btRigidBodyConstructionInfo(0, motionState, shape, localInertia);
    const body: BtRigidBody = new Ammo.btRigidBody(rbInfo);
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
      const refIx = nextCollisionObjectRefId++;
      body.setUserIndex(refIx);
      CollisionObjectRefs.set(refIx, objRef);
    }
    collisionWorld.addRigidBody(body);

    Ammo.destroy(rbInfo);
    // Ammo.destroy(motionState);
    Ammo.destroy(transform);
    return body;
  };

  const buildTrimeshShape = (
    indices: Uint16Array | undefined,
    vertices: Float32Array,
    scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
  ) => {
    // TODO: update IDL and use native indexed triangle mesh
    const trimesh = new Ammo.btTriangleMesh();
    trimesh.preallocateIndices((indices ?? vertices).length);
    trimesh.preallocateVertices(vertices.length);

    const v0 = new Ammo.btVector3();
    const v1 = new Ammo.btVector3();
    const v2 = new Ammo.btVector3();

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
    Ammo.destroy(v0);
    Ammo.destroy(v1);
    Ammo.destroy(v2);

    const shape = new Ammo.btBvhTriangleMeshShape(trimesh, true, true);
    return shape;
  };

  const buildConvexHullShape = (
    indices: Uint16Array | undefined,
    vertices: Float32Array,
    scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
  ) => {
    const hull = new Ammo.btConvexHullShape();

    if (indices) {
      for (let i = 0; i < indices.length; i++) {
        const ix = indices[i] * 3;
        hull.addPoint(btvec3(vertices[ix] * scale.x, vertices[ix + 1] * scale.y, vertices[ix + 2] * scale.z));
      }
    } else {
      for (let i = 0; i < vertices.length; i += 3) {
        hull.addPoint(btvec3(vertices[i] * scale.x, vertices[i + 1] * scale.y, vertices[i + 2] * scale.z));
      }
    }

    return hull;
  };

  const buildCollisionShapeFromMesh = (mesh: THREE.Mesh, extraScale?: THREE.Vector3) => {
    if (mesh.geometry instanceof THREE.BoxGeometry) {
      const halfExtents = btvec3(
        mesh.geometry.parameters.width * mesh.scale.x * (extraScale?.x ?? 1) * 0.5,
        mesh.geometry.parameters.height * mesh.scale.y * (extraScale?.y ?? 1) * 0.5,
        mesh.geometry.parameters.depth * mesh.scale.z * (extraScale?.z ?? 1) * 0.5
      );
      return new Ammo.btBoxShape(halfExtents);
    } else if (
      (mesh.geometry instanceof THREE.SphereGeometry ||
        (mesh.geometry instanceof THREE.IcosahedronGeometry && mesh.geometry.parameters.detail >= 2)) &&
      mesh.scale.x === mesh.scale.y &&
      mesh.scale.y === mesh.scale.z &&
      (!extraScale || (extraScale.x === extraScale.y && extraScale.y === extraScale.z))
    ) {
      const radius = mesh.geometry.parameters.radius * mesh.scale.x * (extraScale?.x ?? 1);
      return new Ammo.btSphereShape(radius);
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

    if (mesh.userData.convexhull) {
      return buildConvexHullShape(indices, vertices, scale);
    }
    return buildTrimeshShape(indices, vertices, scale);
  };

  const addTriMesh = (mesh: THREE.Mesh, colliderType: 'static' | 'kinematic' = 'static') => {
    if (mesh.userData.nocollide || mesh.name.includes('nocollide')) {
      return;
    }

    if (
      (mesh.material instanceof CustomShaderMaterial &&
        mesh.material.materialClass === MaterialClass.Instakill) ||
      mesh.userData.instakill
    ) {
      const collisionObj = addPlayerRegionContactCb({ type: 'mesh', mesh }, () =>
        viz.onInstakillTerrainCollision()
      );
      mesh.userData.collisionObj = collisionObj;
      return;
    }

    const shape = buildCollisionShapeFromMesh(mesh);
    const objRef: CollisionObjectRef = {
      materialClass: mesh.material instanceof CustomShaderMaterial ? mesh.material.materialClass : undefined,
    };
    const rigidBody = addCollisionObject(shape, mesh.position, mesh.quaternion, objRef, colliderType);
    mesh.userData.rigidBody = rigidBody;
  };

  const removeCollisionObject = (collisionObj: BtCollisionObject) => {
    collisionWorld.removeCollisionObject(collisionObj);
    Ammo.destroy(collisionObj);
  };

  const addHeightmapTerrain = (
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
    const terrainDataPtr = Ammo._malloc(4 * heightmapData.length);
    const terrainData = new Float32Array(Ammo.HEAPF32.buffer, terrainDataPtr, heightmapData.length);
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

    const heightfieldShape = new Ammo.btHeightfieldTerrainShape(
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
      btvec3(worldSpaceWidth / (gridResolutionX - 1), 1, worldSpaceLength / (gridResolutionY - 1))
    );

    addCollisionObject(heightfieldShape, new THREE.Vector3(0, 0, 0));
  };

  const addPlayerRegionContactCb: AddPlayerRegionContactCB = (
    region,
    onEnter,
    onLeave,
    minPenetrationDepth = 0.04
  ) => {
    if (!onEnter && !onLeave) {
      throw new Error('Must provide at least one callback');
    }

    const collisionObj = (() => {
      const { shape, transform } = (() => {
        switch (region.type) {
          case 'box': {
            const shape = new Ammo.btBoxShape(
              btvec3(region.halfExtents.x, region.halfExtents.y, region.halfExtents.z)
            );
            const transform = new Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(btvec3(region.pos.x, region.pos.y, region.pos.z));
            if (region.quat) {
              const rot = new Ammo.btQuaternion(region.quat.x, region.quat.y, region.quat.z, region.quat.w);
              transform.setRotation(rot);
              Ammo.destroy(rot);
            }
            return { shape, transform };
          }
          case 'mesh': {
            const { mesh } = region;
            let scale = mesh.scale.clone().multiplyScalar(1 + (region.margin ?? 0));
            if (region.scale) {
              scale = scale.multiply(region.scale);
            }

            const shape = buildCollisionShapeFromMesh(region.mesh, scale);

            const transform = new Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
            if (mesh.quaternion) {
              const rot = new Ammo.btQuaternion(
                mesh.quaternion.x,
                mesh.quaternion.y,
                mesh.quaternion.z,
                mesh.quaternion.w
              );
              transform.setRotation(rot);
              Ammo.destroy(rot);
            }
            return { shape, transform };
          }
          case 'convexHull': {
            const mesh = region.mesh;
            const geometry = mesh.geometry as THREE.BufferGeometry;
            const indices = geometry.index?.array as Uint16Array | undefined;
            const vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
            if (vertices instanceof Uint16Array) {
              throw new Error('GLTF Quantization not yet supported');
            }
            const scale = mesh.scale.clone();
            if (region.scale) {
              scale.multiply(region.scale);
            }

            const shape = buildConvexHullShape(indices, vertices, scale);
            const transform = new Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
            if (mesh.quaternion) {
              const rot = new Ammo.btQuaternion(
                mesh.quaternion.x,
                mesh.quaternion.y,
                mesh.quaternion.z,
                mesh.quaternion.w
              );
              transform.setRotation(rot);
              Ammo.destroy(rot);
            }
            return { shape, transform };
          }
          case 'aabb': {
            const mesh = region.mesh;
            if (region.scale) {
              throw new Error('unimplemented');
            }
            const geometry = mesh.geometry as THREE.BufferGeometry;
            geometry.computeBoundingBox();
            const box = geometry.boundingBox!;
            const transform = new Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(btvec3(mesh.position.x, mesh.position.y, mesh.position.z));

            const shape = new Ammo.btBoxShape(
              btvec3((box.max.x - box.min.x) / 2, (box.max.y - box.min.y) / 2, (box.max.z - box.min.z) / 2)
            );
            return { shape, transform };
          }
          case 'sphere': {
            const shape = new Ammo.btSphereShape(region.radius);
            const transform = new Ammo.btTransform();
            transform.setIdentity();
            transform.setOrigin(btvec3(region.pos.x, region.pos.y, region.pos.z));
            return { shape, transform };
          }
          default:
            region satisfies never;
            throw new Error(`Unhandled region type: ${(region as any).type}.`);
        }
      })();

      const obj = new Ammo.btPairCachingGhostObject();
      obj.setWorldTransform(transform);
      obj.setCollisionShape(shape);
      obj.setCollisionFlags(4); // btCollisionObject::CF_NO_CONTACT_RESPONSE
      Ammo.destroy(transform);
      return obj;
    })();

    // player interacts with static and default filters, so we set the region's ghost object
    // to be of type static and only collide with the player
    collisionWorld.addCollisionObject(
      collisionObj,
      1, // btBroadphaseProxy::StaticFilter,
      32 // btBroadphaseProxy::CharacterFilter
    );

    let isOverlapping = false;
    tickCallbacks.push(() => {
      const numOverlappingObjects = collisionObj.getNumOverlappingObjects();
      // `getNumOverlappingObjects` reports the number of objects that are intersecting in the
      // broadphase, so we have to manually check if the objects are _actually_ colliding.
      const isNowColliding =
        numOverlappingObjects > 0 &&
        collisionWorld.contactPairTestBinary(collisionObj, playerGhostObject, minPenetrationDepth);

      if (isNowColliding && !isOverlapping) {
        isOverlapping = true;
        onEnter?.();
      } else if (!isNowColliding && isOverlapping) {
        isOverlapping = false;
        onLeave?.();
      }
    });

    return collisionObj;
  };

  const addBox = (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const shape = new Ammo.btBoxShape(btvec3(...halfExtents));
    addCollisionObject(shape, new THREE.Vector3(...pos), quat, undefined, colliderType);
  };

  const addCone = (
    pos: THREE.Vector3,
    radius: number,
    height: number,
    quat?: THREE.Quaternion,
    colliderType: 'static' | 'kinematic' = 'static'
  ) => {
    const shape = new Ammo.btConeShape(radius, height);
    addCollisionObject(shape, pos, quat, undefined, colliderType);
  };

  const addCompound = (
    pos: [number, number, number],
    children: {
      type: 'box';
      pos: [number, number, number];
      halfExtents: [number, number, number];
      quat?: THREE.Quaternion;
    }[],
    quat?: THREE.Quaternion
  ) => {
    const parentShape = new Ammo.btCompoundShape(true);

    const childTransform = new Ammo.btTransform();
    for (const child of children) {
      const childShape = (() => {
        if (child.type === 'box') {
          return new Ammo.btBoxShape(btvec3(...child.halfExtents));
        }

        throw new Error('Unimplemented');
      })();

      childTransform.setIdentity();
      childTransform.setOrigin(btvec3(...child.pos));
      if (child.quat) {
        const rot = new Ammo.btQuaternion(child.quat.x, child.quat.y, child.quat.z, child.quat.w);
        childTransform.setRotation(rot);
        Ammo.destroy(rot);
      }
      parentShape.addChildShape(childTransform, childShape);
    }

    addCollisionObject(parentShape, new THREE.Vector3(...pos), quat);

    Ammo.destroy(childTransform);
  };

  const optimize = () => broadphase.optimize();

  const clearCollisionWorld = () => {
    tickCallbacks.length = 0;
    const newCollisionWorld = new Ammo.btDiscreteDynamicsWorld(
      dispatcher,
      broadphase,
      solver,
      collisionConfiguration
    );
    Ammo.destroy(collisionWorld);
    collisionWorld = newCollisionWorld;
  };

  const playerStateGetters: FpPlayerStateGetters = {
    getPlayerPos: () => {
      const pos = playerController.getPosition();
      return [pos.x(), pos.y(), pos.z()];
    },
    getVerticalVelocity: () => playerController.getVerticalVelocity(),
    getVerticalOffset: () => playerController.getVerticalOffset(),
    getIsOnGround: () => playerController.onGround(),
    getJumpAxis: () => {
      const jumpAxis = playerController.getJumpAxis();
      return [jumpAxis.x(), jumpAxis.y(), jumpAxis.z()];
    },
    getExternalVelocity: () => {
      const externalVelocity = playerController.getExternalVelocity();
      return [externalVelocity.x(), externalVelocity.y(), externalVelocity.z()];
    },
    getIsJumping: () => playerController.isJumping() && lastJumpTimeSeconds > dashManager.lastDashTimeSeconds,
    getIsDashing: () => playerController.isJumping() && dashManager.lastDashTimeSeconds > lastJumpTimeSeconds,
  };

  return {
    updateCollisionWorld,
    addTriMesh,
    addBox,
    addCone,
    addCompound,
    addHeightmapTerrain,
    teleportPlayer,
    reset,
    optimize,
    setGravity,
    setFlyMode,
    clearCollisionWorld,
    addPlayerRegionContactCb,
    playerStateGetters,
    removeCollisionObject,
    easyModeMovement,
    registerJumpCb,
    deregisterJumpCb,
    registerDashCb: (cb: (curTimeSeconds: number) => void) => dashManager.registerDashCb(cb),
    deregisterDashCb: (cb: (curTimeSeconds: number) => void) => dashManager.deregisterDashCb(cb),
    btvec3,
  };
};
