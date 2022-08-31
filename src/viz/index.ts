/**
 * Originally adapted from:
 * https://github.com/mrdoob/three.js/blob/master/examples/games_fps.html
 *
 * With many changes and additions.
 */

import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module';
import { Octree } from 'three/examples/jsm/math/Octree.js';
import { Capsule } from 'three/examples/jsm/math/Capsule.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { getSentry } from '../sentry';
import { buildDefaultSceneConfig, ScenesByName, type SceneConfig } from './scenes';
import * as Conf from './conf';

const setupFirstPerson = (
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

  const worldOctree = new Octree();

  const playerColliderHeight = playerConf?.colliderCapsuleSize?.height ?? Conf.DefaultPlayerColliderHeight;
  let playerCollider = new Capsule(
    spawnPos.pos.clone().add(new THREE.Vector3(0, playerColliderHeight, 0)),
    spawnPos.pos.clone().add(new THREE.Vector3(0, playerColliderHeight * 2, 0)),
    playerConf?.colliderCapsuleSize?.radius ?? Conf.DefaultPlayerColliderRadius
  );

  const playerVelocity = new THREE.Vector3();
  const playerDirection = new THREE.Vector3();

  let playerOnFloor = false;

  const keyStates: Record<string, boolean> = {};

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

  function playerCollisions() {
    const result = worldOctree.capsuleIntersect(playerCollider);

    playerOnFloor = false;

    if (result) {
      playerOnFloor = result.normal.y > 0;

      if (!playerOnFloor) {
        playerVelocity.addScaledVector(result.normal, -result.normal.dot(playerVelocity));
      }

      playerCollider.translate(result.normal.multiplyScalar(result.depth));
    }
  }

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

    playerCollisions();

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

  function teleportPlayerIfOob() {
    if (camera.position.y <= -55) {
      const playerColliderHeight = playerCollider.end.clone().sub(playerCollider.start).length();
      const playerColliderRadius = playerCollider.radius;
      playerCollider.start.set(spawnPos.pos.x, spawnPos.pos.y, spawnPos.pos.z);
      playerCollider.end.set(spawnPos.pos.x, spawnPos.pos.y + playerColliderHeight, spawnPos.pos.z);
      console.log(spawnPos.pos.toArray());
      playerCollider.radius = playerColliderRadius;
      playerVelocity.set(0, 0, 0);
      camera.position.copy(playerCollider.end);
      camera.rotation.setFromVector3(spawnPos.rot);
    }
  }

  registerBeforeRenderCb((_curTimeSecs, tDiffSecs) => {
    // we look for collisions in substeps to mitigate the risk of
    // an object traversing another too quickly for detection.

    for (let i = 0; i < STEPS_PER_FRAME; i++) {
      controls(Math.min(0.05, tDiffSecs / STEPS_PER_FRAME));

      updatePlayer(Math.min(0.05, tDiffSecs / STEPS_PER_FRAME));

      teleportPlayerIfOob();
    }
  });

  (window as any).getPos = () => playerCollider.start.toArray();
  (window as any).getRot = () => camera.rotation.toArray();
  (window as any).recordPos = () =>
    JSON.stringify({
      pos: playerCollider.start.toArray(),
      rot: camera.rotation.toArray().slice(0, 3),
    });

  return { setGravity, setJumpVelocity, worldOctree, setPlayerAcceleration };
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
    screen.orientation.lock('landscape');
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
  // console.log({ ext });
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

    let worldOctree: Octree | null = null;
    let setGravity = (_g: number) => {};
    let setJumpVelocity = (_v: number) => {};
    let setPlayerAcceleration = (_onGroundAccPerSec: number, _inAirAccPerSec: number) => {};
    if (sceneConf.viewMode.type === 'firstPerson') {
      const spawnPos = sceneConf.locations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      const fpCtx = setupFirstPerson(
        viz.camera,
        spawnPos,
        viz.registerBeforeRenderCb,
        sceneConf.player,
        sceneConf.gravity
      );
      worldOctree = fpCtx.worldOctree;
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

    const traverseCb = (obj: THREE.Object3D<THREE.Event>) => {
      const children = obj.children;
      obj.children = [];
      if (!(obj instanceof THREE.Group) && !obj.name.includes('nocollide')) {
        worldOctree?.fromGraphNode(obj);
      }
      obj.children = children;
    };
    scene.traverse(traverseCb);

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
