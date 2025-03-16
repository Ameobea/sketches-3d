import * as THREE from 'three';

import type { FirstPersonCtx, VizState } from 'src/viz';
import { type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { buildMaterials } from '../../parkour/regions/pylons/materials';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import Rand, { PRNG } from 'rand-seed';
import type { PopupScreenFocus, InfiniteConfig } from 'src/viz/util';
import InfiniteEndSvelte from 'src/viz/EndScreens/InfiniteEnd.svelte';
import { mount } from 'svelte';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 20, 0),
    rot: new THREE.Vector3(0, 0, 0),
  },
};
let oldestSquareIdx = 0;
let lastRot = new THREE.Vector3(0, 0, -1);

let squares = [
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
];
//additional squares generated
let extraGeneratedCount: number = 0;
const initLevel = async (viz: VizState) => {
  const fpCtx = await new Promise<FirstPersonCtx>(resolve => viz.collisionWorldLoadedCbs.push(resolve));
  //first
  for (let i = 0; i < 2; i++) {
    squares[i].position.set(0, 15, 0 + i * -16);
    viz.scene.add(squares[i]);
    fpCtx.addTriMesh(squares[i]);
  }
  //take in popup input
  let onInfiniteConfigured!: (config: InfiniteConfig) => void;
  const infiniteConfigured = new Promise<InfiniteConfig>(resolve => {
    onInfiniteConfigured = resolve;
  });
  viz.callPopup({ type: 'infinite', cb: onInfiniteConfigured });
  let currentConfig = await infiniteConfigured;
  if (currentConfig.goalLength < 1) {
    currentConfig.goalLength = 1;
  }
  //generate the rest of the tail length
  for (let i = 2; i < currentConfig.activePathLength; i++) {
    squares.push(
      new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 }))
    );
  }
  //build the rest into the world
  const rand = new Rand(currentConfig.seed);
  for (let i = 2; i < currentConfig.activePathLength; i++) {
    let destPoint = generateNextPositionAndRot(rand, squares[i - 1].position, currentConfig.varyingGaps);
    squares[i].position.set(destPoint.x, destPoint.y, destPoint.z);
    viz.scene.add(squares[i]);
    fpCtx.addTriMesh(squares[i]);
  }
  //make last of the initial red
  squares[squares.length - 1].material = new THREE.MeshBasicMaterial({ color: 0xff0000 });
  viz.clock.start();
  setInterval(() => {
    let newestSquareIdx = (oldestSquareIdx - 1 + squares.length) % squares.length;
    viz.camera.position.z;
    let total_dif_to_start =
      Math.abs(squares[oldestSquareIdx].position.x - viz.camera.position.x) +
      Math.abs(squares[oldestSquareIdx].position.y - viz.camera.position.y) +
      Math.abs(squares[oldestSquareIdx].position.z - viz.camera.position.z);
    let total_dif_to_end =
      Math.abs(squares[newestSquareIdx].position.x - viz.camera.position.x) +
      Math.abs(squares[newestSquareIdx].position.y - viz.camera.position.y) +
      Math.abs(squares[newestSquareIdx].position.z - viz.camera.position.z);
    if (total_dif_to_end < total_dif_to_start) {
      let destPoint = generateNextPositionAndRot(
        rand,
        squares[newestSquareIdx].position,
        currentConfig.varyingGaps
      );
      squares[oldestSquareIdx].position.set(destPoint.x, destPoint.y, destPoint.z);
      fpCtx.removeRigidBody(squares[oldestSquareIdx].userData.rigidBody);
      fpCtx.addTriMesh(squares[oldestSquareIdx]);
      squares[oldestSquareIdx].material = new THREE.MeshBasicMaterial({ color: 0xff0000 });
      squares[newestSquareIdx].material = new THREE.MeshBasicMaterial({ color: 0xadd8e6 });
      oldestSquareIdx = (oldestSquareIdx + 1) % squares.length;
      const { position: newPosition, rotation: newRotation } = generateNewSpawnPoint();
      viz.fpCtx!.setSpawnPos(newPosition, newRotation);
      extraGeneratedCount++;
      if (extraGeneratedCount === currentConfig.goalLength) {
        viz.clock.stop;
        console.log('won', extraGeneratedCount, currentConfig.goalLength);
        const target = document.createElement('div');
        document.body.appendChild(target);
        const infiniteEndProps = { currentConfig, time: viz.clock.oldTime };
        const _endDisplay = mount(InfiniteEndSvelte, { target, props: infiniteEndProps });
      }
    }
  }, 250);
};
function generateNewSpawnPoint(): { position: THREE.Vector3; rotation: THREE.Vector3 } {
  //set new spawn
  const newPostion = new THREE.Vector3(
    squares[oldestSquareIdx].position.x,
    squares[oldestSquareIdx].position.y + 10,
    squares[oldestSquareIdx].position.z
  );
  const newRotation = squares[(oldestSquareIdx + 1) % squares.length].position
    .clone()
    .sub(squares[oldestSquareIdx].position);
  const yaw = Math.atan2(-newRotation.x, -newRotation.z);
  const rotationEuler = new THREE.Euler(0, yaw, 0); // Only rotate around Y-axis
  const rotationVector = new THREE.Vector3().setFromEuler(rotationEuler);
  return { position: newPostion, rotation: rotationVector };
}
function generateNextPositionAndRot(
  rand: Rand,
  lastNewestPostion: THREE.Vector3,
  varyingGaps: boolean
): THREE.Vector3 {
  const randDir = new THREE.Vector3(rand.next() * 2 - 1, 0, rand.next() * 2 - 1);
  let distance = 16;
  if (varyingGaps) {
    distance = rand.next() * 11 + 5;
  }
  randDir.normalize();
  //0 -> point
  //starter square aimed at second square

  const curDir = new THREE.Vector3(lastRot.x * 0.5 + randDir.x * 0.5, 0, lastRot.z * 0.5 + randDir.z * 0.5);
  curDir.normalize();
  lastRot = curDir;
  return curDir.clone().multiplyScalar(distance).add(lastNewestPostion);
}
export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);
  viz.registerDistanceMaterialSwap;
  const { checkpointMat, greenMosaic2Material, goldMaterial } = await buildMaterials(viz, loadedWorld);

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  initLevel(viz);

  initPylonsPostprocessing(viz, vizConf);

  function reset() {
    const { position: newPosition, rotation: newRotation } = generateNewSpawnPoint();
    viz.fpCtx!.teleportPlayer(newPosition, newRotation);
    viz.fpCtx!.reset();
  }
  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: {
        enable: true,
        useExternalVelocity: true,
        minDashDelaySeconds: 0,
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    debugPlayerKinematics: true,
    locations: locations,
    legacyLights: false,
    customControlsEntries: [{ label: 'Reset', key: 'f', action: reset }],
    goBackOnLoad: false,
  };
};
