class Empty {}

export interface BtVec3 {
  setValue(x: number, y: number, z: number): void;
  x(): number;
  y(): number;
  z(): number;
}

export interface BtTransform {
  setIdentity(): void;
  getOrigin(): BtVec3;
  setOrigin(vec: BtVec3): void;
  setRotation(quat: BtQuaternion): void;
  getRotation(): BtQuaternion;
  setEulerZYX(x: number, y: number, z: number): void;
}

export interface BtQuaternion {
  setValue(x: number, y: number, z: number, w: number): void;
  setEulerZYX(z: number, y: number, x: number): void;
  x(): number;
  y(): number;
  z(): number;
  w(): number;
}

export interface BtJumpPad {
  setDirection(direction: BtVec3): void;
  setBaseImpulse(impulse: number): void;
  setSpeedScaling(scaling: number): void;
  setCooldownSeconds(seconds: number): void;
  getZoneId(): number;
  setEnabled(enabled: boolean): void;
}

export interface BtBoostZone {
  setDirection(direction: BtVec3): void;
  setStrength(strength: number): void;
  setDirectionalBias(bias: number): void;
  getZoneId(): number;
  setEnabled(enabled: boolean): void;
}

export interface BtSensor {
  getZoneId(): number;
  isOverlapping(): boolean;
  setEnabled(enabled: boolean): void;
}

export enum ZoneEventType {
  SensorEnter = 0,
  SensorLeave = 1,
  JumpPadTriggered = 2,
  BoostZoneEnter = 3,
  BoostZoneExit = 4,
}

export interface BtKinematicCharacterController {
  setMaxPenetrationDepth(depth: number): void;
  setMaxSlope(slope: number): void;
  setStepHeight(height: number): void;
  setJumpSpeed(speed: number): void;
  setGravity(gravity: BtVec3): void;
  setExternalVelocityAirDampingFactor(factor: BtVec3): void;
  setExternalVelocityGroundDampingFactor(factor: BtVec3): void;
  onGround(): boolean;
  jump(velocity: BtVec3): void;
  setWalkDirection(velocity: BtVec3): void;
  getFloorUserIndex(): number;
  warp(position: BtVec3): void;
  getPosition(): BtVec3;
  setExternalVelocity(velocity: BtVec3): void;
  getExternalVelocity(): BtVec3;
  addExternalVelocity(velocity: BtVec3): void;
  setVerticalVelocity(velocity: number): void;
  setOnGround(onGround: boolean): void;
  getVerticalVelocity(): number;
  getVerticalOffset(): number;
  getJumpAxis(): BtVec3;
  isJumping(): boolean;
  resetFall(): void;
  getForcedRotation(): BtQuaternion;
  resetForcedRotation(): void;
  addJumpPad(pad: BtJumpPad): void;
  removeJumpPad(pad: BtJumpPad): void;
  addBoostZone(zone: BtBoostZone): void;
  removeBoostZone(zone: BtBoostZone): void;
  addSensor(sensor: BtSensor): void;
  removeSensor(sensor: BtSensor): void;
  getNumPendingEvents(): number;
  getPendingEventId(index: number): number;
  getPendingEventType(index: number): number;
  clearPendingEvents(): void;
}

export type BtActionInterface = BtKinematicCharacterController;

export interface BtCollisionShape {
  setLocalScaling(scaling: BtVec3): void;
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtConvexShape extends BtCollisionShape {}

export interface BtCollisionObject {
  getWorldTransform(): BtTransform;
  setWorldTransform(transform: BtTransform): void;
  setCollisionShape(shape: BtCollisionShape): void;
  setCollisionFlags(flags: number): void;
  setInterpolationLinearVelocity(linvel: BtVec3): void;
  setInterpolationAngularVelocity(angvel: BtVec3): void;
  getNumOverlappingObjects(): number;
  // getOverlappingObject(index: number): BtCollisionObject;
  setActivationState(state: number): void;
  getCollisionShape(): BtCollisionShape;
}

export interface BtRigidBody extends BtCollisionObject {
  setCollisionFlags(flags: number): void;
  setUserIndex(index: number): void;
  getMotionState(): BtMotionState | undefined | null;
}

export type BtOverlappingPairCallback = Empty;

export interface BtOverlappingPairCache {
  setInternalGhostPairCallback(cb: BtOverlappingPairCallback): void;
}

export interface BtBroadphaseInterface {
  getOverlappingPairCache(): BtOverlappingPairCache;
  optimize(): void;
}

export interface BtDispatcherInfo {
  m_useContinuous: boolean;
}

export interface BtDiscreteDynamicsWorld {
  getBroadphase(): BtBroadphaseInterface;
  addCollisionObject(obj: BtCollisionObject, collisionFilterGroup: number, collisionFilterMask: number): void;
  removeCollisionObject(obj: BtCollisionObject): void;
  addAction(action: BtActionInterface): void;
  setGravity(gravity: BtVec3): void;
  stepSimulation(tDiffSeconds: number, maxSubSteps: number, fixedTimeStep: number): number;
  beginStepSimulation(timeStep: number, maxSubSteps: number, fixedTimeStep: number): number;
  substepSimulation(): void;
  finishStepSimulation(): void;
  computeAndSetInterpolationVelocity(
    body: BtCollisionObject,
    from: BtTransform,
    to: BtTransform,
    dt: number
  ): void;
  addRigidBody(body: BtRigidBody): void;
  // setSynchronizeAllMotionStates(synchronize: boolean): void;
  // getSynchronizeAllMotionStates(): boolean;
  getDispatchInfo(): BtDispatcherInfo;
  contactPairTestBinary(
    colObjA: BtCollisionObject,
    colObjB: BtCollisionObject,
    minPenetrationDepth: number
  ): boolean;
}

export type BtCollisionConfiguration = Empty;

export type BtCollisionDispatcher = Empty;

export type BtSequentialImpulseConstraintSolver = Empty;

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtPairCachingGhostObject extends BtCollisionObject {}

export interface BtMotionState {
  setWorldTransform(transform: BtTransform): void;
  getWorldTransform(): BtTransform;
}

export type BtRigidBodyConstructionInfo = Empty;

export interface BtTriangleMesh {
  preallocateIndices(numIndices: number): void;
  preallocateVertices(numVertices: number): void;
  addTriangle(v0: BtVec3, v1: BtVec3, v2: BtVec3): void;
}

export interface BtConvexHullShape extends BtCollisionShape {
  addPoint(point: BtVec3, recalculateLocalAabb?: boolean): void;
}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtConcaveShape extends BtCollisionShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtHeightfieldTerrainShape extends BtConcaveShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtCapsuleShape extends BtCollisionShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtCylinderShape extends BtCollisionShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtBoxShape extends BtCollisionShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtConeShape extends BtCollisionShape {}

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface BtSphereShape extends BtCollisionShape {}

export interface BtCompoundShape extends BtCollisionShape {
  addChildShape(transform: BtTransform, shape: BtCollisionShape): void;
}

export interface AmmoInterface {
  destroy(obj: any): void;
  btDefaultCollisionConfiguration: new () => BtCollisionConfiguration;
  btCollisionDispatcher: new (config: BtCollisionConfiguration) => BtCollisionDispatcher;
  btDbvtBroadphase: new () => BtBroadphaseInterface;
  btSequentialImpulseConstraintSolver: new () => BtSequentialImpulseConstraintSolver;
  btTransform: new () => BtTransform;
  btPairCachingGhostObject: new () => BtPairCachingGhostObject;
  btDiscreteDynamicsWorld: new (
    dispatcher: BtCollisionDispatcher,
    broadphase: BtBroadphaseInterface,
    solver: BtSequentialImpulseConstraintSolver,
    collisionConfiguration: BtCollisionConfiguration
  ) => BtDiscreteDynamicsWorld;
  btGhostPairCallback: new () => BtOverlappingPairCallback;
  btCapsuleShape: new (radius: number, height: number) => BtCapsuleShape;
  btCylinderShape: new (halfExtents: BtVec3) => BtCylinderShape;
  btSphereShape: new (radius: number) => BtSphereShape;
  btJumpPad: new (
    ghostObject: BtPairCachingGhostObject,
    zoneId: number,
    baseImpulse: number,
    speedScaling: number,
    cooldownSeconds: number
  ) => BtJumpPad;
  btBoostZone: new (
    ghostObject: BtPairCachingGhostObject,
    zoneId: number,
    strength: number,
    directionalBias: number
  ) => BtBoostZone;
  btSensor: new (
    ghostObject: BtPairCachingGhostObject,
    zoneId: number,
    minPenetrationDepth: number
  ) => BtSensor;
  btKinematicCharacterController: new (
    ghostObject: BtPairCachingGhostObject,
    shape: BtConvexShape,
    stepHeight: number,
    up?: BtVec3
  ) => BtKinematicCharacterController;
  btQuaternion: new (x: number, y: number, z: number, w: number) => BtQuaternion;
  btDefaultMotionState: new (transform: BtTransform) => BtMotionState;
  btRigidBodyConstructionInfo: new (
    mass: number,
    motionState: BtMotionState,
    collisionShape: BtCollisionShape,
    localInertia?: BtVec3
  ) => BtRigidBodyConstructionInfo;
  btTriangleMesh: new () => BtTriangleMesh;
  btVector3: new () => BtVec3;
  btBvhTriangleMeshShape: new (
    trimesh: BtTriangleMesh,
    useQuantizedAabbCompression: boolean,
    buildBVH: boolean
  ) => BtCollisionShape;
  btConvexHullShape: new () => BtConvexHullShape;
  _malloc: (size: number) => number;
  HEAPF32: Float32Array;
  btHeightfieldTerrainShape: new (
    gridResolutionX: number,
    gridResolutionY: number,
    terrainDataPtr: number,
    heightScale: number,
    minHeight: number,
    maxHeight: number,
    upAxis: number,
    flipQuadEdges: boolean
  ) => BtHeightfieldTerrainShape;
  btBoxShape: new (halfExtents: BtVec3) => BtBoxShape;
  btConeShape: new (radius: number, height: number) => BtConeShape;
  btCompoundShape: new (enableDynamicAabbTree: boolean) => BtCompoundShape;
  btRigidBody: {
    new (info: BtRigidBodyConstructionInfo): BtRigidBody;
    prototype: {
      upcast: (obj: BtCollisionObject) => BtRigidBody | null;
    };
  };
}
