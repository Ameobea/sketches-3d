/**
 * Originally adapted from:
 * https://github.com/mrdoob/three.js/blob/master/examples/games_fps.html
 *
 * With many changes and additions.
 */

import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { buildDefaultSceneConfig, ScenesByName, type SceneConfig } from './scenes';
import * as Conf from './conf';
import { getAmmoJS } from './collision';
import { Inventory } from './inventory/Inventory';

const initBulletPhysics = (
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
  const collisionWorld = new Ammo.btDiscreteDynamicsWorld(
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
  collisionWorld.setGravity(btvec3(0, -gravity, 0));

  let lastJumpTimeSeconds = 0;
  const MIN_JUMP_DELAY_SECONDS = 0.25; // TODO: make configurable
  let lastBoostTimeSeconds = 0;
  const MIN_BOOST_DELAY_SECONDS = 0.85; // TODO: make configurable
  let boostNeedsGroundTouch = false;

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
      if (curTimeSeconds - lastBoostTimeSeconds > MIN_BOOST_DELAY_SECONDS && !boostNeedsGroundTouch) {
        playerController.jump(btvec3(origForwardDir.x * 16, origForwardDir.y * 16, origForwardDir.z * 16));
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

  const addTriMesh = (mesh: THREE.Mesh | 'DONE') => {
    if (mesh === 'DONE') {
      broadphase.optimize();
      return;
    }

    if (mesh.userData.nocollide || mesh.name.includes('nocollide')) {
      return;
    }

    const geometry = mesh.geometry as THREE.BufferGeometry;
    let vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
    if (!geometry.index?.array) {
      console.error('Mesh has no index array; not adding to collision world', mesh);
      return;
    }
    const indices = geometry.index!.array as Uint16Array;
    if (vertices instanceof Uint16Array) {
      throw new Error('GLTF Quantization not yet supported');
      // console.log(geometry.attributes.position);
      // TODO
    }
    const scale = mesh.scale;
    const pos = mesh.position;
    const quat = mesh.quaternion;

    const transform = new Ammo.btTransform();
    transform.setIdentity();
    transform.setOrigin(btvec3(pos.x, pos.y, pos.z));
    const rot = new Ammo.btQuaternion(quat.x, quat.y, quat.z, quat.w);
    transform.setRotation(rot);
    Ammo.destroy(rot);

    const buildTrimeshShape = () => {
      // TODO: update IDL and use native indexed triangle mesh
      const trimesh = new Ammo.btTriangleMesh();
      trimesh.preallocateIndices(indices.length);
      trimesh.preallocateVertices(vertices.length);

      const v0 = new Ammo.btVector3();
      const v1 = new Ammo.btVector3();
      const v2 = new Ammo.btVector3();

      for (let i = 0; i < indices.length; i += 3) {
        const i0 = indices[i] * 3;
        const i1 = indices[i + 1] * 3;
        const i2 = indices[i + 2] * 3;
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
    // Ammo.destroy(trimesh);
    Ammo.destroy(transform);
  };

  return { updateCollisionWorld, addTriMesh, teleportPlayer };
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
  gravity: number | undefined,
  inlineConsole: InlineConsole | null | undefined,
  enableDash: boolean
) => {
  let GRAVITY = gravity ?? 40;
  let JUMP_VELOCITY = playerConf?.jumpVelocity ?? 20;
  let ON_FLOOR_ACCELERATION_PER_SECOND = playerConf?.movementAccelPerSecond?.onGround ?? 40;

  const keyStates: Record<string, boolean> = {};

  const playerColliderHeight = playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight;
  const playerColliderRadius = playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius;

  const Ammo = await getAmmoJS();
  const { updateCollisionWorld, addTriMesh, teleportPlayer } = await initBulletPhysics(
    camera,
    keyStates,
    Ammo,
    spawnPos,
    GRAVITY,
    JUMP_VELOCITY,
    playerColliderRadius,
    playerColliderHeight,
    ON_FLOOR_ACCELERATION_PER_SECOND,
    enableDash
  );

  document.addEventListener('keydown', event => {
    if (inlineConsole?.isOpen) {
      return;
    }

    keyStates[event.code] = true;
  });

  document.addEventListener('keyup', event => {
    if (inlineConsole?.isOpen) {
      return;
    }

    keyStates[event.code] = false;
  });

  document.addEventListener('mousedown', () => {
    document.body.requestPointerLock();
  });

  document.body.addEventListener('mousemove', event => {
    if (document.pointerLockElement === document.body) {
      camera.rotation.y -= event.movementX / 500;
      camera.rotation.x -= event.movementY / 500;

      // Clamp the camera's rotation to the range of -PI/2 to PI/2
      // This is so the camera doesn't flip upside down
      camera.rotation.x = Math.max(-Math.PI / 2, Math.min(Math.PI / 2, camera.rotation.x));
    }
  });

  (window as any).tp = (posName: string) => {
    const location = locations[posName];
    if (location) {
      teleportPlayer(location.pos, location.rot);
    } else {
      console.log('No location found');
    }
  };

  window.onbeforeunload = function () {
    localStorage.backPos = (window as any).recordPos();
  };

  (window as any).back = () => {
    const backPos = localStorage.backPos;
    if (!backPos) {
      console.log('No back position found');
      return;
    }

    const { pos, rot } = JSON.parse(backPos);
    teleportPlayer(new THREE.Vector3(pos[0], pos[1], pos[2]), new THREE.Vector3(rot[0], rot[1], rot[2]));
  };

  function teleportPlayerIfOOB() {
    if (camera.position.y <= -55) {
      teleportPlayer(spawnPos.pos, spawnPos.rot);
    }
  }

  registerBeforeRenderCb((curTimeSecs, tDiffSecs) => {
    const newPlayerPos = updateCollisionWorld(curTimeSecs, tDiffSecs);
    newPlayerPos.y += 0.5 * playerColliderHeight;
    camera.position.copy(newPlayerPos);

    teleportPlayerIfOOB();
  });

  (window as any).getPos = () =>
    camera.position
      .clone()
      .sub(new THREE.Vector3(0, playerColliderHeight / 2, 0))
      .toArray();
  (window as any).getRot = () => camera.rotation.toArray();
  (window as any).recordPos = () =>
    JSON.stringify({
      pos: (window as any).getPos(),
      rot: camera.rotation.toArray().slice(0, 3),
    });

  return { addTriMesh, teleportPlayer };
};

const disposeScene = (scene: THREE.Scene) =>
  scene.traverse(o => {
    if (o instanceof THREE.Mesh) {
      o.geometry?.dispose();
      if (Array.isArray(o.material)) {
        o.material.forEach(m => m.dispose());
      } else {
        o.material?.dispose();
      }
    }
  });

class InlineConsole {
  private elem: HTMLDivElement;
  public isOpen = false;
  private keydownCB: (e: KeyboardEvent) => void;

  constructor() {
    this.keydownCB = event => {
      if (!this.isOpen) {
        if (event.key === '/') {
          this.open();
        }
        return;
      }

      if (event.key === '/' || event.key === 'Escape') {
        this.close();
        return;
      } else if (event.key === 'Enter') {
        this.eval();
        this.close();
        return;
      } else if (event.key.length === 1) {
        this.elem.innerText =
          this.elem.innerText + (event.shiftKey ? event.key.toUpperCase() : event.key.toLowerCase());
      } else if (event.key === 'Backspace') {
        this.elem.innerText = this.elem.innerText.slice(0, -1);
      }
    };
    document.addEventListener('keydown', this.keydownCB);

    const elem = document.createElement('div');
    elem.id = 'inline-console';
    elem.style.position = 'absolute';
    elem.style.bottom = '8px';
    elem.style.left = '8px';
    elem.style.width = '100%';
    elem.style.height = '20px';
    elem.style.fontFamily = '"Oxygen Mono", "Input", "Hack", monospace';
    elem.style.display = 'none';
    elem.style.backgroundColor = 'rgba(0, 0, 0, 0.5)';
    elem.style.color = '#eee';
    elem.style.zIndex = '100';
    document.body.appendChild(elem);
    this.elem = elem;
  }

  open = () => {
    this.isOpen = true;
    this.elem.style.display = 'block';
  };

  close = () => {
    this.isOpen = false;
    this.elem.style.display = 'none';
  };

  eval = () => {
    const content = this.elem.innerText;
    this.elem.innerText = '';
    try {
      console.log(eval(content));
    } catch (e) {
      console.error(e);
    }
  };

  public destroy() {
    this.elem.remove();
    document.removeEventListener('keydown', this.keydownCB);
  }
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

  const camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, 3_000);
  camera.matrixAutoUpdate = true;
  camera.rotation.order = 'YXZ';

  const renderer = new THREE.WebGLRenderer({
    antialias: false,
    powerPreference: 'high-performance',
    stencil: false,
  });
  (window as any).renderer = renderer;
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
  // renderer.shadowMap.autoUpdate = false;
  // renderer.shadowMap.needsUpdate = true;

  const stats = Stats.default();
  stats.domElement.style.position = 'absolute';
  stats.domElement.style.top = '0px';

  const resizeCbs: (() => void)[] = [];
  const beforeRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];
  const afterRenderCbs: ((curTimeSeconds: number, tDiffSeconds: number) => void)[] = [];

  function onWindowResize() {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();

    renderer.setSize(window.innerWidth, window.innerHeight);

    resizeCbs.forEach(cb => cb());
  }
  window.addEventListener('resize', onWindowResize);

  const registerResizeCb = (cb: () => void) => resizeCbs.push(cb);
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

  let renderOverride: ((timeDiffSeconds: number) => void) | null = null;
  const setRenderOverride = (cb: ((timeDiffSeconds: number) => void) | null) => {
    renderOverride = cb;
  };

  let animateHandle: number = 0;
  function animate() {
    if (isBlurred) {
      animateHandle = requestAnimationFrame(animate);
      return;
    }

    const deltaTime = clock.getDelta();
    const curTimeSeconds = clock.getElapsedTime();

    beforeRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    if (renderOverride) {
      renderOverride(deltaTime);
    } else {
      renderer.render(scene, camera);
    }

    afterRenderCbs.forEach(cb => cb(curTimeSeconds, deltaTime));

    stats.update();

    animateHandle = requestAnimationFrame(animate);
  }

  const onDestroy = () => {
    (window as any).lastPos = (window as any).recordPos();
    renderer.dispose();
    beforeRenderCbs.length = 0;
    afterRenderCbs.length = 0;
    if (animateHandle) {
      cancelAnimationFrame(animateHandle);
    }
    disposeScene(scene);
    console.clear();
  };

  const inventory = new Inventory();

  return {
    camera,
    renderer,
    stats,
    scene,
    animate,
    registerResizeCb,
    registerBeforeRenderCb,
    unregisterBeforeRenderCb,
    registerDistanceMaterialSwap,
    registerAfterRenderCb,
    unregisterAfterRenderCb,
    onDestroy,
    setRenderOverride,
    inventory,
  };
};

export type VizState = ReturnType<typeof buildViz>;

export const initViz = (container: HTMLElement, providedSceneName: string = Conf.DefaultSceneName) => {
  const viz = buildViz();

  container.appendChild(viz.renderer.domElement);
  // if (window.location.href.includes('localhost')) {
  container.appendChild(viz.stats.domElement);
  // }

  const inlineConsole = window.location.href.includes('localhost') || true ? new InlineConsole() : null;

  const loader = new GLTFLoader().setPath('/');

  const sceneDef = ScenesByName[providedSceneName];
  if (!sceneDef) {
    throw new Error(`No scene found for name ${providedSceneName}`);
  }
  const { sceneName, sceneLoader: getSceneLoader, gltfName: providedGLTFName } = sceneDef;
  const gltfName = providedGLTFName === undefined ? 'dream' : providedGLTFName;

  const gltfLoadedCB = async (gltf: { scenes: THREE.Group[] }) => {
    providedSceneName = providedSceneName.toLowerCase();

    let scene =
      gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase()) || new THREE.Group();

    const sceneLoader = await getSceneLoader();
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene)) ?? {}),
    };

    if (sceneConf.renderOverride) {
      viz.setRenderOverride(sceneConf.renderOverride);
    }

    (window as any).locations = () => Object.keys(sceneConf.locations);

    if (sceneConf.enableInventory) {
      // TODO: set up inventory CBs
    }

    let addTriMesh: ((mesh: THREE.Mesh | 'DONE') => void) | null = null;
    if (sceneConf.viewMode.type === 'firstPerson') {
      const spawnPos = (window as any).lastPos
        ? (() => {
            const lastPos = JSON.parse((window as any).lastPos);
            return {
              pos: new THREE.Vector3(lastPos.pos[0], lastPos.pos[1], lastPos.pos[2]),
              rot: new THREE.Vector3(lastPos.rot[0], lastPos.rot[1], lastPos.rot[2]),
            };
          })()
        : sceneConf.locations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      const fpCtx = await setupFirstPerson(
        sceneConf.locations,
        viz.camera,
        spawnPos,
        viz.registerBeforeRenderCb,
        sceneConf.player,
        sceneConf.gravity,
        inlineConsole,
        sceneConf.player?.enableDash ?? true
      );
      addTriMesh = fpCtx.addTriMesh;
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
      if (child instanceof THREE.Mesh && !child.name.includes('background')) {
        if (child.userData.noLight) {
          child.castShadow = false;
          child.receiveShadow = false;
        } else {
          child.castShadow = true;
          child.receiveShadow = true;
        }
        if (child.userData.noCastShadow) {
          child.castShadow = false;
        }
        if (child.userData.noReceiveShadow) {
          child.receiveShadow = false;
        }
      }
    });

    viz.animate();
  };

  if (gltfName) {
    loader.load(`${gltfName}.gltf`, gltfLoadedCB);
  } else {
    gltfLoadedCB({ scenes: [] });
  }

  return {
    destroy() {
      viz.onDestroy();
      inlineConsole?.destroy();
    },
  };
};
