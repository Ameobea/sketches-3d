/**
 * Originally adapted from:
 * https://github.com/mrdoob/three.js/blob/master/examples/games_fps.html
 *
 * With many changes and additions.
 */

import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module';
// import { Octree } from 'three/examples/jsm/math/Octree.js';
import { Capsule } from 'three/examples/jsm/math/Capsule.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { getSentry } from '../sentry';
import { buildDefaultSceneConfig, ScenesByName, type SceneConfig } from './scenes';
import * as Conf from './conf';
import { getAmmoJS } from './collision';

const initBulletPhysics = (
  camera: THREE.Camera,
  keyStates: Record<string, boolean>,
  Ammo: any,
  spawnPos: { pos: THREE.Vector3; rot: THREE.Vector3 },
  gravity: number,
  jumpSpeed: number,
  playerColliderRadius: number,
  playerColliderHeight: number,
  playerMoveSpeed: number
) => {
  const collisionConfiguration = new Ammo.btDefaultCollisionConfiguration();
  const dispatcher = new Ammo.btCollisionDispatcher(collisionConfiguration);
  const broadphase = new Ammo.btDbvtBroadphase();
  const solver = new Ammo.btSequentialImpulseConstraintSolver();
  const collisionWorld = new Ammo.btDiscreteDynamicsWorld(
    dispatcher,
    broadphase,
    solver,
    collisionConfiguration
  );

  const playerInitialTransform = new Ammo.btTransform();
  playerInitialTransform.setIdentity();
  playerInitialTransform.setOrigin(
    new Ammo.btVector3(spawnPos.pos.x, spawnPos.pos.y + playerColliderHeight, spawnPos.pos.z)
  );
  const playerGhostObject = new Ammo.btPairCachingGhostObject();
  playerGhostObject.setWorldTransform(playerInitialTransform);
  collisionWorld
    .getBroadphase()
    .getOverlappingPairCache()
    .setInternalGhostPairCallback(new Ammo.btGhostPairCallback());
  const playerCapsule = new Ammo.btCapsuleShape(playerColliderRadius, playerColliderHeight);
  playerGhostObject.setCollisionShape(playerCapsule);
  playerGhostObject.setCollisionFlags(16); // btCollisionObject::CF_CHARACTER_OBJECT

  const playerController = new Ammo.btKinematicCharacterController(
    playerGhostObject,
    playerCapsule,
    0.35, // step height; TODO: make this configurable
    new Ammo.btVector3(0, 1, 0)
  );
  playerController.setMaxPenetrationDepth(0.055);
  playerController.setJumpSpeed(jumpSpeed);

  collisionWorld.addCollisionObject(
    playerGhostObject,
    32, // btBroadphaseProxy::CharacterFilter
    1 | 2 // btBroadphaseProxy::StaticFilter | btBroadphaseProxy::DefaultFilter
  );
  collisionWorld.addAction(playerController);
  collisionWorld.setGravity(new Ammo.btVector3(0, -gravity, 0));

  let lastJumpTimeSeconds = 0;
  const MIN_JUMP_DELAY_SECONDS = 0.25; // TODO: make configurable

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

    const walkDirection = new THREE.Vector3();
    if (keyStates['KeyW']) walkDirection.add(forwardDir);
    if (keyStates['KeyS']) walkDirection.sub(forwardDir);
    if (keyStates['KeyA']) walkDirection.add(leftDir);
    if (keyStates['KeyD']) walkDirection.sub(leftDir);
    if (keyStates['Space'] && playerController.onGround()) {
      if (curTimeSeconds - lastJumpTimeSeconds > MIN_JUMP_DELAY_SECONDS) {
        playerController
          .jump
          // new Ammo.btVector3(origForwardDir.x * 16, origForwardDir.y * 16, origForwardDir.z * 16)
          ();
        lastJumpTimeSeconds = curTimeSeconds;
      }
    }

    const walkSpeed = playerMoveSpeed * (1 / 160);
    const walkDirBulletVector = new Ammo.btVector3(
      walkDirection.x * walkSpeed,
      walkDirection.y * walkSpeed,
      walkDirection.z * walkSpeed
    );
    // console.log(
    //   'walkDirBulletVector',
    //   walkDirBulletVector.x(),
    //   walkDirBulletVector.y(),
    //   walkDirBulletVector.z()
    // );
    // TODO: Check out `setVelocityForTimeInterval` compared to this
    playerController.setWalkDirection(walkDirBulletVector);

    collisionWorld.stepSimulation(tDiffSeconds, 20, 1 / 300);

    const newPlayerTransform = playerGhostObject.getWorldTransform();
    const newPlayerPos = newPlayerTransform.getOrigin();
    // console.log('newPlayerPos', newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());

    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };

  const addTriMesh = (mesh: THREE.Mesh | 'DONE') => {
    if (mesh === 'DONE') {
      broadphase.optimize();
      return;
    }

    // debug only
    // const boxShape = new Ammo.btBoxShape(new Ammo.btVector3(100, 10, 100));
    // const boxTransform = new Ammo.btTransform();
    // boxTransform.setIdentity();
    // boxTransform.setOrigin(new Ammo.btVector3(0, 0, 0));
    // const boxMotionState = new Ammo.btDefaultMotionState(boxTransform);
    // const boxRBInfo = new Ammo.btRigidBodyConstructionInfo(
    //   0,
    //   boxMotionState,
    //   boxShape,
    //   new Ammo.btVector3(0, 0, 0)
    // );
    // const boxRB = new Ammo.btRigidBody(boxRBInfo);
    // boxRB.setCollisionFlags(1);
    // collisionWorld.addRigidBody(boxRB);
    // return;

    const geometry = mesh.geometry as THREE.BufferGeometry;
    const vertices = geometry.attributes.position.array as Float32Array;
    const indices = geometry.index!.array as Uint16Array;
    const scale = mesh.scale;
    const pos = mesh.position;
    const quat = mesh.quaternion;

    const transform = new Ammo.btTransform();
    transform.setIdentity();
    transform.setOrigin(new Ammo.btVector3(pos.x, pos.y, pos.z));
    transform.setRotation(new Ammo.btQuaternion(quat.x, quat.y, quat.z, quat.w));

    // TODO: update IDL and use native indexed triangle mesh
    const trimesh = new Ammo.btTriangleMesh();
    trimesh.preallocateIndices(indices.length);
    trimesh.preallocateVertices(vertices.length);
    for (let i = 0; i < indices.length; i += 3) {
      const i0 = indices[i] * 3;
      const i1 = indices[i + 1] * 3;
      const i2 = indices[i + 2] * 3;
      const v0 = new Ammo.btVector3(
        vertices[i0] * scale.x,
        vertices[i0 + 1] * scale.y,
        vertices[i0 + 2] * scale.z
      );
      const v1 = new Ammo.btVector3(
        vertices[i1] * scale.x,
        vertices[i1 + 1] * scale.y,
        vertices[i1 + 2] * scale.z
      );
      const v2 = new Ammo.btVector3(
        vertices[i2] * scale.x,
        vertices[i2 + 1] * scale.y,
        vertices[i2 + 2] * scale.z
      );
      // TODO: compute triangle area and log about ones that are too big or too small
      // Area of triangles should be <10 units, as suggested by user guide
      // Should be greater than 0.05 or something like that too probably
      trimesh.addTriangle(v0, v1, v2);
    }

    const shape = new Ammo.btBvhTriangleMeshShape(trimesh, true, true);
    // Add the object as static, so it doesn't move but still collides
    const motionState = new Ammo.btDefaultMotionState(transform);
    const localInertia = new Ammo.btVector3(0, 0, 0);
    const rbInfo = new Ammo.btRigidBodyConstructionInfo(0, motionState, shape, localInertia);
    const body = new Ammo.btRigidBody(rbInfo);
    // body.setFriction(1);
    body.setCollisionFlags(1); // btCollisionObject::CF_STATIC_OBJECT
    if (!body.isStaticObject()) {
      throw new Error('body is not static');
    }
    collisionWorld.addRigidBody(body);
    // console.log('Added trimesh', body);
  };

  return { updateCollisionWorld, addTriMesh };
};

const setupFirstPerson = async (
  locations: SceneConfig['locations'],
  camera: THREE.Camera,
  spawnPos: {
    pos: THREE.Vector3;
    rot: THREE.Vector3;
  },
  registerBeforeRenderCb: (cb: (curTimeSecs: number, tDiffSecs: number) => void) => void,
  playerConf: SceneConfig['player'],
  gravity: number | undefined
) => {
  let GRAVITY = gravity ?? 40;
  let JUMP_VELOCITY = playerConf?.jumpVelocity ?? 20;
  let ON_FLOOR_ACCELERATION_PER_SECOND = playerConf?.movementAccelPerSecond?.onGround ?? 40;
  let IN_AIR_ACCELERATION_PER_SECOND = playerConf?.movementAccelPerSecond?.inAir ?? 20;
  const STEPS_PER_FRAME = 20;

  const keyStates: Record<string, boolean> = {};

  const Ammo = await getAmmoJS();
  const { updateCollisionWorld, addTriMesh } = await initBulletPhysics(
    camera,
    keyStates,
    Ammo,
    spawnPos,
    GRAVITY,
    JUMP_VELOCITY,
    playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius,
    playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight,
    ON_FLOOR_ACCELERATION_PER_SECOND
  );

  const setGravity = (gravity: number) => {
    GRAVITY = gravity;
  };
  const setJumpVelocity = (jumpVelocity: number) => {
    JUMP_VELOCITY = jumpVelocity;
  };
  const setPlayerAcceleration = (onGroundAccPerSec: number, inAirAccPerSec: number) => {
    ON_FLOOR_ACCELERATION_PER_SECOND = onGroundAccPerSec;
    IN_AIR_ACCELERATION_PER_SECOND = inAirAccPerSec;
  };

  const playerColliderHeight = playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight;
  let playerCollider = new Capsule(
    spawnPos.pos.clone().add(new THREE.Vector3(0, playerColliderHeight, 0)),
    spawnPos.pos.clone().add(new THREE.Vector3(0, playerColliderHeight * 2, 0)),
    playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius
  );

  const playerVelocity = new THREE.Vector3();
  const playerDirection = new THREE.Vector3();

  let playerOnFloor = false;

  document.addEventListener('keydown', event => {
    if (event.key === 'e') {
      document.exitPointerLock();
    }

    keyStates[event.code] = true;
  });

  document.addEventListener('keyup', event => {
    keyStates[event.code] = false;
  });

  document.addEventListener('mousedown', () => {
    document.body.requestPointerLock();
  });

  document.body.addEventListener('mousemove', event => {
    if (document.pointerLockElement === document.body) {
      camera.rotation.y -= event.movementX / 500;
      camera.rotation.x -= event.movementY / 500;
    }
  });

  // function playerCollisions(timeDiffSeconds: number) {
  //   const result = worldOctree.capsuleIntersect(playerCollider);

  //   playerOnFloor = false;

  //   if (result) {
  //     playerOnFloor = result.normal.y > 0;

  //     if (!playerOnFloor) {
  //       playerVelocity.addScaledVector(result.normal, -result.normal.dot(playerVelocity));
  //     }

  //     playerCollider.translate(result.normal.multiplyScalar(result.depth));
  //   }
  // }

  function updatePlayer(deltaTime: number) {
    let damping = Math.exp(-4 * deltaTime) - 1;

    if (!playerOnFloor) {
      playerVelocity.y -= GRAVITY * deltaTime;

      // small air resistance
      damping *= 0.1;
    }

    playerVelocity.addScaledVector(playerVelocity, damping);

    // if (playerVelocity.lengthSq() > 0.1) {
    const deltaPosition = playerVelocity.clone().multiplyScalar(deltaTime);
    playerCollider.translate(deltaPosition);
    // }

    // playerCollisions(deltaTime);

    camera.position.copy(playerCollider.end);
  }

  function getForwardVector() {
    camera.getWorldDirection(playerDirection);
    playerDirection.y = 0;
    playerDirection.normalize();

    return playerDirection;
  }

  function getSideVector() {
    camera.getWorldDirection(playerDirection);
    playerDirection.y = 0;
    playerDirection.normalize();
    playerDirection.cross(camera.up);

    return playerDirection;
  }

  function controls(deltaTime: number) {
    // gives a bit of air control
    const speedDelta =
      deltaTime * (playerOnFloor ? ON_FLOOR_ACCELERATION_PER_SECOND : IN_AIR_ACCELERATION_PER_SECOND);

    if (keyStates['KeyW']) {
      playerVelocity.add(getForwardVector().multiplyScalar(speedDelta));
    }

    if (keyStates['KeyS']) {
      playerVelocity.add(getForwardVector().multiplyScalar(-speedDelta));
    }

    if (keyStates['KeyA']) {
      playerVelocity.add(getSideVector().multiplyScalar(-speedDelta));
    }

    if (keyStates['KeyD']) {
      playerVelocity.add(getSideVector().multiplyScalar(speedDelta));
    }

    if (playerOnFloor) {
      if (keyStates['Space']) {
        playerVelocity.y = JUMP_VELOCITY;
      }
    }
  }

  const teleportPlayer = (pos: THREE.Vector3, rot?: THREE.Vector3) => {
    const playerColliderHeight = playerCollider.end.clone().sub(playerCollider.start).length();
    const playerColliderRadius = playerCollider.radius;
    playerCollider.start.set(pos.x, pos.y, pos.z);
    playerCollider.end.set(pos.x, pos.y + playerColliderHeight, pos.z);
    playerCollider.radius = playerColliderRadius;
    playerVelocity.set(0, 0, 0);
    camera.position.copy(playerCollider.end);
    if (rot) {
      camera.rotation.setFromVector3(rot);
    }
  };
  (window as any).tp = (posName: string) => {
    const location = locations[posName];
    if (location) {
      teleportPlayer(location.pos, location.rot);
    } else {
      console.log('No location found');
    }
  };

  function teleportPlayerIfOob() {
    if (camera.position.y <= -55) {
      teleportPlayer(spawnPos.pos, spawnPos.rot);
    }
  }

  registerBeforeRenderCb((curTimeSecs, tDiffSecs) => {
    // we look for collisions in substeps to mitigate the risk of
    // an object traversing another too quickly for detection.

    // for (let i = 0; i < STEPS_PER_FRAME; i++) {
    //   controls(Math.min(0.05, tDiffSecs / STEPS_PER_FRAME));

    //   updatePlayer(Math.min(0.05, tDiffSecs / STEPS_PER_FRAME));

    //   teleportPlayerIfOob();
    // }

    const newPlayerPos = updateCollisionWorld(curTimeSecs, tDiffSecs);
    newPlayerPos.y += playerColliderHeight;
    camera.position.copy(newPlayerPos);
  });

  (window as any).getPos = () => playerCollider.start.toArray();
  (window as any).getRot = () => camera.rotation.toArray();
  (window as any).recordPos = () =>
    JSON.stringify({
      pos: playerCollider.start.toArray(),
      rot: camera.rotation.toArray().slice(0, 3),
    });

  return { setGravity, setJumpVelocity, setPlayerAcceleration, addTriMesh };
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
};

export const buildViz = () => {
  try {
    screen.orientation.lock('landscape').catch(() => 0);
  } catch (err) {
    // pass
  }

  const enableShadows = true;

  const clock = new THREE.Clock();

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x020202);

  const camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.18, 10_000);
  camera.matrixAutoUpdate = true;
  camera.rotation.order = 'YXZ';

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  // const ext = renderer.getContext().getExtension('WEBGL_compressed_texture_s3tc');
  const gl = renderer.getContext();
  const fragDerivExt = gl.getExtension('OES_standard_derivatives');
  if (fragDerivExt) {
    renderer.getContext().hint(fragDerivExt.FRAGMENT_SHADER_DERIVATIVE_HINT_OES, gl.NICEST);
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(window.innerWidth, window.innerHeight);
  renderer.shadowMap.enabled = enableShadows;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.shadowMap.autoUpdate = false;
  renderer.shadowMap.needsUpdate = true;

  const stats = Stats.default();
  stats.domElement.style.position = 'absolute';
  stats.domElement.style.top = '0px';

  window.addEventListener('resize', onWindowResize);

  function onWindowResize() {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();

    renderer.setSize(window.innerWidth, window.innerHeight);
  }

  const beforeRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];
  const afterRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];

  const registerBeforeRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) =>
    beforeRenderCbs.push(cb);
  const unregisterBeforeRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = beforeRenderCbs.indexOf(cb);
    if (idx !== -1) {
      beforeRenderCbs.splice(idx, 1);
    }
  };

  const registerAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) =>
    afterRenderCbs.push(cb);
  const unregisterAfterRenderCb = (cb: (curTimeSeconds: number, tDiffSeconds: number) => void) => {
    const idx = afterRenderCbs.indexOf(cb);
    if (idx !== -1) {
      afterRenderCbs.splice(idx, 1);
    }
  };

  const distanceSwapEntries: {
    mesh: THREE.Mesh;
    baseMat: THREE.Material;
    replacementMat: THREE.Material;
    distance: number;
  }[] = [];
  registerBeforeRenderCb(() => {
    for (const { mesh, baseMat, replacementMat, distance } of distanceSwapEntries) {
      const distanceToCamera = camera.position.distanceTo(mesh.position);
      if (distanceToCamera < distance) {
        if (mesh.material !== baseMat) {
          console.log('swapping back to close mat', mesh.name);
        }
        mesh.material = baseMat;
      } else {
        if (mesh.material !== replacementMat) {
          console.log('swapping to far mat', mesh.name);
        }
        mesh.material = replacementMat;
      }
    }
  });

  const registerDistanceMaterialSwap = (mesh: THREE.Mesh, replacementMat: THREE.Material, distance = 150) => {
    const baseMat = mesh.material;
    if (!baseMat || Array.isArray(baseMat)) {
      throw new Error('Mesh must have a single material');
    }
    distanceSwapEntries.push({ mesh, baseMat, replacementMat, distance });
  };

  let isBlurred = false;
  let clockStopTime = 0;
  window.addEventListener('blur', () => {
    isBlurred = true;
    clockStopTime = clock.getElapsedTime();
    clock.stop();
  });
  window.addEventListener('focus', () => {
    isBlurred = false;
    clock.start();
    clock.elapsedTime = clockStopTime;
  });

  function animate() {
    if (isBlurred) {
      requestAnimationFrame(animate);
      return;
    }

    const deltaTime = clock.getDelta();
    const curTimeSeconds = clock.getElapsedTime();

    beforeRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    renderer.render(scene, camera);

    afterRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    stats.update();

    requestAnimationFrame(animate);
  }

  const onDestroy = () => {
    renderer.dispose();
    beforeRenderCbs.length = 0;
    afterRenderCbs.length = 0;
  };

  return {
    camera,
    renderer,
    stats,
    scene,
    animate,
    registerBeforeRenderCb,
    unregisterBeforeRenderCb,
    registerDistanceMaterialSwap,
    registerAfterRenderCb,
    unregisterAfterRenderCb,
    onDestroy,
  };
};

export type VizState = ReturnType<typeof buildViz>;

export const initViz = (container: HTMLElement, providedSceneName: string = Conf.DefaultSceneName) => {
  const viz = buildViz();

  container.appendChild(viz.renderer.domElement);
  // if (window.location.href.includes('localhost')) {
  container.appendChild(viz.stats.domElement);
  // }

  const loader = new GLTFLoader().setPath('/');

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }
  const { sceneName, sceneLoader: getSceneLoader, gltfName = 'dream' } = sceneDef;

  loader.load(`${gltfName}.gltf`, async gltf => {
    providedSceneName = providedSceneName.toLowerCase();

    const scene =
      gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group();

    const sceneLoader = await getSceneLoader();
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene)) ?? {}),
    };

    (window as any).locations = () => Object.keys(sceneConf.locations);

    let addTriMesh: ((mesh: THREE.Mesh | 'DONE') => void) | null = null;
    let setGravity = (_g: number) => {};
    let setJumpVelocity = (_v: number) => {};
    let setPlayerAcceleration = (_onGroundAccPerSec: number, _inAirAccPerSec: number) => {};
    if (sceneConf.viewMode.type === 'firstPerson') {
      const spawnPos = sceneConf.locations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      const fpCtx = await setupFirstPerson(
        sceneConf.locations,
        viz.camera,
        spawnPos,
        viz.registerBeforeRenderCb,
        sceneConf.player,
        sceneConf.gravity
      );
      addTriMesh = fpCtx.addTriMesh;
      setGravity = fpCtx.setGravity;
      setJumpVelocity = fpCtx.setJumpVelocity;
      setPlayerAcceleration = fpCtx.setPlayerAcceleration;
    } else if (sceneConf.viewMode.type === 'orbit') {
      await setupOrbitControls(
        viz.renderer.domElement,
        viz.camera,
        sceneConf.viewMode.pos,
        sceneConf.viewMode.target
      );
    }

    viz.scene.add(scene);

    if (sceneConf.debugPos) {
      const posDisplayElem = document.createElement('div');
      posDisplayElem.style.position = 'absolute';
      posDisplayElem.style.top = '0px';
      posDisplayElem.style.right = '0px';
      posDisplayElem.style.color = 'white';
      posDisplayElem.style.fontSize = '12px';
      posDisplayElem.style.fontFamily = 'monospace';
      posDisplayElem.style.padding = '4px';
      posDisplayElem.style.backgroundColor = 'rgba(0, 0, 0, 0.5)';
      posDisplayElem.style.zIndex = '1';
      container.appendChild(posDisplayElem);

      viz.registerBeforeRenderCb(() => {
        const x = viz.camera.position.x.toFixed(2);
        const y = viz.camera.position.y.toFixed(2);
        const z = viz.camera.position.z.toFixed(2);
        posDisplayElem.innerText = `${x}, ${y}, ${z}`;
      });
    }

    if (addTriMesh) {
      const traverseCb = (obj: THREE.Object3D<THREE.Event>) => {
        const children = obj.children;
        obj.children = [];
        if (!(obj instanceof THREE.Group) && !obj.name.includes('nocollide') && !obj.name.endsWith('far')) {
          // worldOctree!.fromGraphNode(obj);
          if (obj instanceof THREE.Mesh) {
            addTriMesh!(obj);
          }
        }
        obj.children = children;
      };
      scene.traverse(traverseCb);
      addTriMesh('DONE');
    }

    // TODO: Combine with above
    scene.traverse(child => {
      if (child instanceof THREE.Mesh) {
        child.castShadow = true;
        child.receiveShadow = true;
      }
    });

    viz.animate();
  });

  return {
    destroy() {
      viz.onDestroy();
    },
  };
};
