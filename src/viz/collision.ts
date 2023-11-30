import { get } from 'svelte/store';
import * as THREE from 'three';

import type { SfxManager } from './audio/SfxManager.js';
import type { FpPlayerStateGetters } from './index.js';
import {
  type DashConfig,
  DefaultDashConfig,
  DefaultMoveSpeed,
  type PlayerMoveSpeed,
} from './scenes/index.js';
import { CustomShaderMaterial } from './shaders/customShader';
import { MaterialClass } from './shaders/customShader.js';
import { assertUnreachable, mergeDeep } from './util.js';

let ammojs: Promise<any> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) return ammojs;
  ammojs = import('../ammojs/ammo.wasm.js').then(mod => mod.Ammo.apply({}));
  return ammojs;
};

let playerController: any = null;
let btvec3: (x: number, y: number, z: number) => any = () => {
  throw new Error('btvec3 not initialized');
};

const initBtvec3Scratch = (Ammo: any) => {
  const scratchVec = new Ammo.btVector3();
  btvec3 = (x: number, y: number, z: number) => {
    scratchVec.setValue(x, y, z);
    return scratchVec;
  };
};

class DashManager {
  private config: DashConfig;
  private lastDashTimeSeconds = 0;
  /**
   * `true` if the player has not touched the ground since they last dashed
   */
  private dashNeedsGroundTouch = false;

  static mergeConfig(config: Partial<DashConfig> | undefined): DashConfig {
    if (!config) {
      return DefaultDashConfig;
    }
    return mergeDeep({ ...DefaultDashConfig }, config);
  }

  constructor(config: Partial<DashConfig> | undefined) {
    this.config = DashManager.mergeConfig(config);
  }

  private dashInner(origForwardDir: THREE.Vector3, curTimeSeconds: number) {
    playerController.jump(
      btvec3(
        origForwardDir.x * this.config.dashMagnitude,
        origForwardDir.y * this.config.dashMagnitude,
        origForwardDir.z * this.config.dashMagnitude
      )
    );
    this.lastDashTimeSeconds = curTimeSeconds;
    this.dashNeedsGroundTouch = true;

    if (this.config.chargeConfig) {
      this.config.chargeConfig.curCharges.update(n => n - 1);
    }
  }

  public tick(curTimeSeconds: number, onGround: boolean) {
    if (
      curTimeSeconds - this.lastDashTimeSeconds > this.config.minDashDelaySeconds &&
      this.dashNeedsGroundTouch &&
      onGround
    ) {
      this.dashNeedsGroundTouch = false;
    }
  }

  /**
   * Attempts to dash if the necessary conditions are met.  Returns `true` if the dash was actually performed.
   */
  public tryDash(curTimeSeconds: number, isFlyMode: boolean, origForwardDir: THREE.Vector3): boolean {
    if (!this.config.enable) {
      return false;
    }

    // check if not enough time since last dash
    if (curTimeSeconds - this.lastDashTimeSeconds <= this.config.minDashDelaySeconds) {
      return false;
    }

    if (this.config.chargeConfig) {
      if (get(this.config.chargeConfig.curCharges) <= 0) {
        return false;
      }
    }

    if (this.dashNeedsGroundTouch && !isFlyMode) {
      return false;
    }

    this.dashInner(origForwardDir, curTimeSeconds);
    return true;
  }
}

export type ContactRegion =
  | { type: 'box'; pos: THREE.Vector3; halfExtents: THREE.Vector3; quat?: THREE.Quaternion }
  | { type: 'mesh'; mesh: THREE.Mesh; margin?: number; scale?: THREE.Vector3 }
  | { type: 'convexHull'; mesh: THREE.Mesh; scale?: THREE.Vector3 }
  | { type: 'aabb'; mesh: THREE.Mesh; scale?: THREE.Vector3 };

export type AddPlayerRegionContactCB = (
  region: ContactRegion,
  onEnter?: () => void,
  onLeave?: () => void
) => void;

interface BulletPhysicsArgs {
  camera: THREE.Camera;
  keyStates: Record<string, boolean>;
  Ammo: any;
  spawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 };
  gravity: number;
  jumpSpeed: number;
  playerColliderRadius: number;
  playerColliderHeight: number;
  playerMoveSpeed: PlayerMoveSpeed | undefined;
  dashConfig: Partial<DashConfig> | undefined;
  sfxManager: SfxManager;
}

export const initBulletPhysics = ({
  camera,
  keyStates,
  Ammo,
  spawnPos,
  gravity,
  jumpSpeed,
  playerColliderRadius,
  playerColliderHeight,
  playerMoveSpeed = DefaultMoveSpeed,
  dashConfig = DefaultDashConfig,
  sfxManager,
}: BulletPhysicsArgs) => {
  const collisionConfiguration = new Ammo.btDefaultCollisionConfiguration();
  const dispatcher = new Ammo.btCollisionDispatcher(collisionConfiguration);
  const broadphase = new Ammo.btDbvtBroadphase();
  const solver = new Ammo.btSequentialImpulseConstraintSolver();
  let collisionWorld = new Ammo.btDiscreteDynamicsWorld(
    dispatcher,
    broadphase,
    solver,
    collisionConfiguration
  );

  initBtvec3Scratch(Ammo);

  const playerInitialTransform = new Ammo.btTransform();
  playerInitialTransform.setIdentity();
  playerInitialTransform.setOrigin(
    btvec3(spawnPos.pos.x, spawnPos.pos.y + playerColliderHeight, spawnPos.pos.z)
  );
  const playerGhostObject = new Ammo.btPairCachingGhostObject();
  playerGhostObject.setWorldTransform(playerInitialTransform);
  Ammo.destroy(playerInitialTransform);
  collisionWorld
    .getBroadphase()
    .getOverlappingPairCache()
    .setInternalGhostPairCallback(new Ammo.btGhostPairCallback());
  const playerCapsule = new Ammo.btCapsuleShape(playerColliderRadius, playerColliderHeight);
  playerGhostObject.setCollisionShape(playerCapsule);
  playerGhostObject.setCollisionFlags(16); // btCollisionObject::CF_CHARACTER_OBJECT

  // \/ This is vital for making the physics work without bad bugs like falling through floors randomly.
  //
  // After deconstructing what the kinematic character controller does internally, I've worked out that it
  // tries to push the player both up and down by this amount every tick of the simulation.
  //
  // If it's too big, the player tends to clip through geometry or stuff like that.
  const STEP_HEIGHT = 0.05;
  const MAX_SLOPE_RADS = 0.8;
  // \/ This is a very important config item for the physics engine.  Setting it too high will result in
  // the player vibrating and janking out when pushing into corners and similar.  Setting too low causes
  // weird issues where the player slides around on the floor or clips through geometry.
  const MAX_PENETRATION_DEPTH = 0.075;
  playerController = new Ammo.btKinematicCharacterController(
    playerGhostObject,
    playerCapsule,
    STEP_HEIGHT,
    btvec3(0, 1, 0)
  );
  playerController.setMaxPenetrationDepth(MAX_PENETRATION_DEPTH);
  playerController.setMaxSlope(MAX_SLOPE_RADS);
  playerController.setStepHeight(STEP_HEIGHT);
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

  let lastJumpTimeSeconds = 0;
  const MIN_JUMP_DELAY_SECONDS = 0.25; // TODO: make configurable
  let lastDashTimeSeconds = 0;

  const dashManager = new DashManager(dashConfig);

  let isFlyMode = false;
  const setFlyMode = (newIsFlyMode?: boolean) => {
    if (newIsFlyMode === isFlyMode) {
      return;
    }

    isFlyMode = newIsFlyMode ?? !isFlyMode;
    setGravity(isFlyMode ? 0 : gravity);
  };

  const tickCallbacks: ((tDiffSeconds: number) => void)[] = [];
  /**
   * Returns the new position of the player.
   */
  const moveDirection = new THREE.Vector3();
  let isWalking = false;
  const upDir = new THREE.Vector3(0, 1, 0);
  let forwardDir = new THREE.Vector3();
  let leftDir = new THREE.Vector3();
  const updateCollisionWorld = (curTimeSeconds: number, tDiffSeconds: number): THREE.Vector3 => {
    forwardDir = camera.getWorldDirection(forwardDir).normalize();
    const origForwardDir = forwardDir.clone();
    leftDir = leftDir.crossVectors(upDir, forwardDir).normalize();
    // Adjust `forwardDir` to be horizontal.
    forwardDir = forwardDir.crossVectors(leftDir, upDir).normalize();

    const wasOnGround = playerController.onGround();

    moveDirection.set(0, 0, 0);
    if (keyStates['KeyW']) moveDirection.add(forwardDir);
    if (keyStates['KeyS']) moveDirection.sub(forwardDir);
    if (keyStates['KeyA']) moveDirection.add(leftDir);
    if (keyStates['KeyD']) moveDirection.sub(leftDir);
    if (keyStates['Space'] && wasOnGround) {
      if (curTimeSeconds - lastJumpTimeSeconds > MIN_JUMP_DELAY_SECONDS) {
        playerController.jump(
          btvec3(moveDirection.x * (jumpSpeed * 0.18), jumpSpeed, moveDirection.z * (jumpSpeed * 0.18))
        );
        lastJumpTimeSeconds = curTimeSeconds;
      }
    }

    const wasWalking = isWalking;
    isWalking = moveDirection.x !== 0 || moveDirection.y !== 0 || moveDirection.z !== 0;
    if (wasWalking && !isWalking) {
      sfxManager.onWalkStop();
    } else if (!wasWalking && isWalking) {
      sfxManager.onWalkStart(MaterialClass.Default);
    }

    dashManager.tick(curTimeSeconds, wasOnGround);

    if (keyStates['ShiftLeft'] || keyStates['ShiftRight']) {
      dashManager.tryDash(curTimeSeconds, isFlyMode, origForwardDir);
    }

    const moveSpeedPerSecond = wasOnGround ? playerMoveSpeed.onGround : playerMoveSpeed.inAir;
    const moveSpeedPerTick = moveSpeedPerSecond * (1 / 160);
    const walkDirBulletVector = btvec3(
      moveDirection.x * moveSpeedPerTick,
      moveDirection.y * moveSpeedPerTick,
      moveDirection.z * moveSpeedPerTick
    );
    playerController.setWalkDirection(walkDirBulletVector);

    collisionWorld.stepSimulation(tDiffSeconds, 20, 1 / 160);

    const newPlayerTransform = playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();

    const nowOnGround = playerController.onGround();
    if (!wasOnGround && nowOnGround) {
      const landedOnObjectIx: number = playerController.getFloorUserIndex();
      const landedOnObject = CollisionObjectRefs.get(landedOnObjectIx);
      const materialClass = landedOnObject?.materialClass ?? MaterialClass.Default;
      sfxManager.onPlayerLand(materialClass);
    }

    for (const cb of tickCallbacks) {
      cb(tDiffSeconds);
    }

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  const teleportPlayer = (pos: THREE.Vector3, rot?: THREE.Vector3) => {
    playerController.warp(btvec3(pos.x, pos.y + playerColliderHeight, pos.z));
    if (rot) {
      camera.rotation.setFromVector3(rot);
    }
  };

  teleportPlayer(spawnPos.pos, spawnPos.rot);

  interface CollisionObjectRef {
    materialClass?: MaterialClass;
  }
  let nextCollisionObjectRefId = 0;
  const CollisionObjectRefs: Map<number, CollisionObjectRef> = new Map();

  const addStaticShape = (
    shape: any,
    pos: THREE.Vector3,
    quat: THREE.Quaternion = new THREE.Quaternion(),
    objRef?: CollisionObjectRef
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
    const body = new Ammo.btRigidBody(rbInfo);
    body.setCollisionFlags(1); // btCollisionObject::CF_STATIC_OBJECT
    if (!body.isStaticObject()) {
      throw new Error('body is not static');
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
    for (let i = 0; i < (indices ?? vertices).length; i += 3) {
      const point = (() => {
        if (indices) {
          return btvec3(
            vertices[indices[i]] * scale.x,
            vertices[indices[i + 1]] * scale.y,
            vertices[indices[i + 2]] * scale.z
          );
        } else {
          return btvec3(vertices[i] * scale.x, vertices[i + 1] * scale.y, vertices[i + 2] * scale.z);
        }
      })();
      hull.addPoint(point);
    }
    return hull;
  };

  const addTriMesh = (mesh: THREE.Mesh) => {
    if (mesh.userData.nocollide || mesh.name.includes('nocollide')) {
      return;
    }

    const geometry = mesh.geometry as THREE.BufferGeometry;
    let vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
    // if (!geometry.index?.array) {
    //   console.error('Mesh has no index array; not adding to collision world', mesh);
    //   return;
    // }
    const indices = geometry.index?.array as Uint16Array | undefined;
    if (vertices instanceof Uint16Array) {
      throw new Error('GLTF Quantization not yet supported');
    }
    const scale = mesh.scale;

    const shape = mesh.userData.convexhull
      ? buildConvexHullShape(indices, vertices, scale)
      : buildTrimeshShape(indices, vertices, scale);
    const objRef: CollisionObjectRef = {
      materialClass: mesh.material instanceof CustomShaderMaterial ? mesh.material.materialClass : undefined,
    };
    const rigidBody = addStaticShape(shape, mesh.position, mesh.quaternion, objRef);
    mesh.userData.rigidBody = rigidBody;
  };

  const removeRigidBody = (rigidBody: any) => {
    collisionWorld.removeRigidBody(rigidBody);
    Ammo.destroy(rigidBody);
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

    addStaticShape(heightfieldShape, new THREE.Vector3(0, 0, 0));
  };

  const addPlayerRegionContactCb: AddPlayerRegionContactCB = (region, onEnter, onLeave) => {
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
            const mesh = region.mesh;
            const geometry = mesh.geometry as THREE.BufferGeometry;
            let vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
            const indices = geometry.index?.array as Uint16Array | undefined;
            if (vertices instanceof Uint16Array) {
              throw new Error('GLTF Quantization not yet supported');
            }
            let scale = mesh.scale.clone().multiplyScalar(1 + (region.margin ?? 0));
            if (region.scale) {
              scale = scale.multiply(region.scale);
            }

            const shape = buildTrimeshShape(indices, vertices, scale);
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
          default:
            return assertUnreachable(region);
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
      if (numOverlappingObjects > 0 && !isOverlapping) {
        isOverlapping = true;
        onEnter?.();
      } else if (numOverlappingObjects === 0 && isOverlapping) {
        isOverlapping = false;
        onLeave?.();
      }
    });
  };

  const addBox = (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion
  ) => {
    const shape = new Ammo.btBoxShape(btvec3(...halfExtents));
    addStaticShape(shape, new THREE.Vector3(...pos), quat);
  };

  const addCone = (pos: THREE.Vector3, radius: number, height: number, quat?: THREE.Quaternion) => {
    const shape = new Ammo.btConeShape(radius, height);
    addStaticShape(shape, pos, quat);
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

    addStaticShape(parentShape, new THREE.Vector3(...pos), quat);

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
    getVerticalVelocity: () => playerController.getVerticalVelocity(),
    getIsOnGround: () => playerController.onGround(),
    getIsJumping: () => playerController.isJumping() && lastJumpTimeSeconds > lastDashTimeSeconds,
    getIsDashing: () => playerController.isJumping() && lastDashTimeSeconds > lastJumpTimeSeconds,
  };

  const setMoveSpeed = (newMoveSpeed: PlayerMoveSpeed) => {
    playerMoveSpeed = newMoveSpeed;
  };

  return {
    updateCollisionWorld,
    addTriMesh,
    addBox,
    addCone,
    addCompound,
    addHeightmapTerrain,
    teleportPlayer,
    optimize,
    setGravity,
    setFlyMode,
    clearCollisionWorld,
    addPlayerRegionContactCb,
    playerStateGetters,
    removeRigidBody,
    setMoveSpeed,
  };
};
