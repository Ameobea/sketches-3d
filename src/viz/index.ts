/**
 * THIS IS ALL TAKEN DIRECTLY (with small changes) FROM:
 * https://github.com/mrdoob/three.js/blob/master/examples/games_fps.html
 */

import * as THREE from 'three';
import * as Stats from 'three/examples/jsm/libs/stats.module';
import { Octree } from 'three/examples/jsm/math/Octree.js';
import { Capsule } from 'three/examples/jsm/math/Capsule.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';

import { getSentry } from '../sentry';
import { buildDefaultSceneConfig, ScenesByName } from './scenes';
import * as Conf from './conf';

const setupFirstPerson = (
  camera: THREE.Camera,
  spawnPos: {
    pos: THREE.Vector3;
    rot: THREE.Vector3;
  },
  registerBeforeRenderCb: (cb: (curTimeSecs: number, tDiffSecs: number) => void) => void
) => {
  const GRAVITY = 40;
  const STEPS_PER_FRAME = 5;

  const worldOctree = new Octree();

  const playerCollider = new Capsule(
    spawnPos.pos.clone(),
    spawnPos.pos.clone().add(new THREE.Vector3(0, Conf.PlayerColliderHeight, 0)),
    Conf.PlayerColliderRadius
  );

  const playerVelocity = new THREE.Vector3();
  const playerDirection = new THREE.Vector3();

  let playerOnFloor = false;

  const keyStates = {};

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

  function updatePlayer(deltaTime) {
    let damping = Math.exp(-4 * deltaTime) - 1;

    if (!playerOnFloor) {
      playerVelocity.y -= GRAVITY * deltaTime;

      // small air resistance
      damping *= 0.1;
    }

    playerVelocity.addScaledVector(playerVelocity, damping);

    const deltaPosition = playerVelocity.clone().multiplyScalar(deltaTime);
    playerCollider.translate(deltaPosition);

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

  function controls(deltaTime) {
    // gives a bit of air control
    const speedDelta = deltaTime * (playerOnFloor ? 40 : 9);

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
        playerVelocity.y = 20;
      }
    }
  }

  function teleportPlayerIfOob() {
    if (camera.position.y <= -55) {
      playerCollider.start.set(spawnPos.pos.x, spawnPos.pos.y, spawnPos.pos.z);
      playerCollider.end.set(spawnPos.pos.x, spawnPos.pos.y + Conf.PlayerColliderHeight, spawnPos.pos.z);
      playerCollider.radius = Conf.PlayerColliderRadius;
      camera.position.copy(playerCollider.end);
      camera.rotation.set(0, 0, 0);
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

  return worldOctree;
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

  const camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, 1000);
  camera.rotation.order = 'YXZ';

  const renderer = new THREE.WebGLRenderer({ antialias: true });
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
    console.log('focus');
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

  loader.load('dream.gltf', async gltf => {
    providedSceneName = providedSceneName.toLowerCase();

    const { sceneName, sceneLoader: getSceneLoader } = ScenesByName[providedSceneName];
    const scene = gltf.scenes.find(scene => scene.name.toLowerCase() === sceneName.toLowerCase());

    if (!scene) {
      alert(`scene ${sceneName} not found in loaded gltf`);
      throw new Error(`Scene ${sceneName} not found in loaded gltf`);
    }

    const sceneLoader = await getSceneLoader();
    const sceneConf = {
      ...buildDefaultSceneConfig(),
      ...((await sceneLoader(viz, scene)) ?? {}),
    };

    let worldOctree: Octree | null = null;
    if (sceneConf.viewMode.type === 'firstPerson') {
      const spawnPos = sceneConf.locations[sceneConf.spawnLocation];
      viz.camera.rotation.setFromVector3(spawnPos.rot, 'YXZ');
      worldOctree = setupFirstPerson(viz.camera, spawnPos, viz.registerBeforeRenderCb);
    } else if (sceneConf.viewMode.type === 'orbit') {
      await setupOrbitControls(
        viz.renderer.domElement,
        viz.camera,
        sceneConf.viewMode.pos,
        sceneConf.viewMode.target
      );
    }

    viz.scene.add(scene);

    (window as any).ctx = viz.renderer.getContext();
    (window as any).renderer = viz.renderer;

    viz.scene.add(scene);

    viz.scene.getObjectByName('instance')?.removeFromParent();

    const traverseCb = (obj: THREE.Object3D<THREE.Event>) => {
      const children = obj.children;
      obj.children = [];
      if (!(obj instanceof THREE.Group)) {
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
