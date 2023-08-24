import * as THREE from 'three';

let ammojs: Promise<any> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) return ammojs;
  ammojs = import('../ammojs/ammo.wasm.js').then(mod => mod.Ammo.apply({}));
  return ammojs;
};

export const initBulletPhysics = (
  camera: THREE.Camera,
  keyStates: Record<string, boolean>,
  Ammo: any,
  spawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 },
  gravity: number,
  jumpSpeed: number,
  playerColliderRadius: number,
  playerColliderHeight: number,
  playerMoveSpeed: number,
  enableDash: boolean
) => {
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

  const scratchVec = new Ammo.btVector3();
  const btvec3 = (x: number, y: number, z: number) => {
    scratchVec.setValue(x, y, z);
    return scratchVec;
  };

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
  // \/ This is a very important config item for the physics engine.  Setting it too high will result in
  // the player vibrating and janking out when pushing into corners and similar.  Setting too low causes
  // weird issues where the player slides around on the floor or clips through geometry.
  const MAX_PENETRATION_DEPTH = 0.075;
  const playerController = new Ammo.btKinematicCharacterController(
    playerGhostObject,
    playerCapsule,
    STEP_HEIGHT,
    btvec3(0, 1, 0)
  );
  playerController.setMaxPenetrationDepth(MAX_PENETRATION_DEPTH);
  playerController.setMaxSlope(0.8); // ~45 degrees
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
  let lastBoostTimeSeconds = 0;
  let MIN_BOOST_DELAY_SECONDS = 0.85; // TODO: make configurable
  let BOOST_MAGNITUDE = 16; // TODO: make configurable
  let boostNeedsGroundTouch = false;

  let isFlyMode = false;
  const setFlyMode = (newIsFlyMode?: boolean) => {
    if (newIsFlyMode === isFlyMode) {
      return;
    }

    isFlyMode = newIsFlyMode ?? !isFlyMode;
    MIN_BOOST_DELAY_SECONDS = isFlyMode ? -1 : 0.85;
    setGravity(isFlyMode ? 0 : gravity);
  };

  /**
   * Returns the new position of the player.
   */
  const updateCollisionWorld = (curTimeSeconds: number, tDiffSeconds: number): THREE.Vector3 => {
    let forwardDir = camera.getWorldDirection(new THREE.Vector3()).normalize();
    const origForwardDir = forwardDir.clone();
    const upDir = new THREE.Vector3(0, 1, 0);
    const leftDir = new THREE.Vector3().crossVectors(upDir, forwardDir).normalize();
    // Adjust `forwardDir` to be horizontal.
    forwardDir = new THREE.Vector3().crossVectors(leftDir, upDir).normalize();

    const playerOnGround = playerController.onGround();

    const walkDirection = new THREE.Vector3();
    if (keyStates['KeyW']) walkDirection.add(forwardDir);
    if (keyStates['KeyS']) walkDirection.sub(forwardDir);
    if (keyStates['KeyA']) walkDirection.add(leftDir);
    if (keyStates['KeyD']) walkDirection.sub(leftDir);
    if (keyStates['Space'] && playerOnGround) {
      if (curTimeSeconds - lastJumpTimeSeconds > MIN_JUMP_DELAY_SECONDS) {
        playerController.jump(
          btvec3(walkDirection.x * (jumpSpeed * 0.18), jumpSpeed, walkDirection.z * (jumpSpeed * 0.18))
        );
        lastJumpTimeSeconds = curTimeSeconds;
      }
    }

    if ((keyStates['ShiftLeft'] || keyStates['ShiftRight']) && enableDash) {
      if (
        curTimeSeconds - lastBoostTimeSeconds > MIN_BOOST_DELAY_SECONDS &&
        (isFlyMode || !boostNeedsGroundTouch)
      ) {
        playerController.jump(
          btvec3(
            origForwardDir.x * BOOST_MAGNITUDE,
            origForwardDir.y * BOOST_MAGNITUDE,
            origForwardDir.z * BOOST_MAGNITUDE
          )
        );
        lastBoostTimeSeconds = curTimeSeconds;
        boostNeedsGroundTouch = true;
      }
    }

    if (
      curTimeSeconds - lastBoostTimeSeconds > MIN_BOOST_DELAY_SECONDS &&
      boostNeedsGroundTouch &&
      playerOnGround
    ) {
      boostNeedsGroundTouch = false;
    }

    const walkSpeed = playerMoveSpeed * (1 / 160);
    const walkDirBulletVector = btvec3(
      walkDirection.x * walkSpeed,
      walkDirection.y * walkSpeed,
      walkDirection.z * walkSpeed
    );
    playerController.setWalkDirection(walkDirBulletVector);

    collisionWorld.stepSimulation(tDiffSeconds, 20, 1 / 160);

    const newPlayerTransform = playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  const teleportPlayer = (pos: THREE.Vector3, rot?: THREE.Vector3) => {
    playerController.warp(btvec3(pos.x, pos.y + playerColliderHeight, pos.z));
    // camera.position.copy(pos.clone().add(new THREE.Vector3(0, playerColliderHeight, 0)));
    if (rot) {
      camera.rotation.setFromVector3(rot);
    }
  };

  teleportPlayer(spawnPos.pos, spawnPos.rot);

  const addStaticShape = (
    shape: any,
    pos: THREE.Vector3,
    quat: THREE.Quaternion = new THREE.Quaternion()
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
    collisionWorld.addRigidBody(body);

    Ammo.destroy(rbInfo);
    // Ammo.destroy(motionState);
    Ammo.destroy(transform);
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

    const buildTrimeshShape = () => {
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

    const buildConvexHullShape = () => {
      const hull = new Ammo.btConvexHullShape();
      for (let i = 0; i < vertices.length; i += 3) {
        hull.addPoint(btvec3(vertices[i] * scale.x, vertices[i + 1] * scale.y, vertices[i + 2] * scale.z));
      }
      return hull;
    };

    const shape = mesh.userData.convexhull ? buildConvexHullShape() : buildTrimeshShape();
    addStaticShape(shape, mesh.position, mesh.quaternion);
  };

  const addBox = (
    pos: [number, number, number],
    halfExtents: [number, number, number],
    quat?: THREE.Quaternion
  ) => {
    const shape = new Ammo.btBoxShape(btvec3(...halfExtents));
    addStaticShape(shape, new THREE.Vector3(...pos), quat);
  };

  const optimize = () => broadphase.optimize();

  const clearCollisionWorld = () => {
    const newCollisionWorld = new Ammo.btDiscreteDynamicsWorld(
      dispatcher,
      broadphase,
      solver,
      collisionConfiguration
    );
    Ammo.destroy(collisionWorld);
    collisionWorld = newCollisionWorld;
  };

  return {
    updateCollisionWorld,
    addTriMesh,
    addBox,
    teleportPlayer,
    optimize,
    setGravity,
    setFlyMode,
    clearCollisionWorld,
  };
};
