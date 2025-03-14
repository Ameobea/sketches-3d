import * as THREE from 'three';

import type { FirstPersonCtx, VizState } from 'src/viz';
import { type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { Score, type ScoreThresholds } from '../../parkour/TimeDisplay.svelte';
import { buildMaterials } from '../../parkour/regions/pylons/materials';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { ParkourManager } from '../../parkour/ParkourManager.svelte';
import { randInt } from 'three/src/math/MathUtils.js';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 20, 0),
    rot: new THREE.Vector3(0, 0, 0),
  },
};
let oldestSquareIdx = 0;
let squares = [
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
  new THREE.Mesh(new THREE.BoxGeometry(4, 4, 4), new THREE.MeshBasicMaterial({ color: 0xadd8e6 })),
];
let lastDir = new THREE.Vector3(0, 0, -1);
const initLevel = async (viz: VizState) => {
  const fpCtx = await new Promise<FirstPersonCtx>(resolve => viz.collisionWorldLoadedCbs.push(resolve));

  for (let i = 0; i < squares.length; i++) {
    squares[i].position.set(0, 15, 0 + i * -16);
    viz.scene.add(squares[i]);
    fpCtx.addTriMesh(squares[i]);
    console.log('length of collision:', viz.collisionWorldLoadedCbs.length);
  }

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
      const randDir = new THREE.Vector3(Math.random() * 2 - 1, 0, Math.random() * 2 - 1);
      randDir.normalize();
      console.log('rand');
      console.log(randDir.toArray().toLocaleString());
      console.log('last');
      console.log(lastDir.toArray().toLocaleString());

      const curDir = new THREE.Vector3(
        lastDir.x * 0.5 + randDir.x * 0.5,
        0,
        lastDir.z * 0.5 + randDir.z * 0.5
      );
      curDir.normalize();
      console.log('cur');
      console.log(curDir.toArray().toLocaleString());
      const destPoint = curDir.clone().multiplyScalar(16).add(squares[newestSquareIdx].position);
      // let nextPosition: { x: number; y: number; z: number } = {
      //   x: squares[newestSquareIdx].position.x,
      //   y: squares[newestSquareIdx].position.y,
      //   z: squares[newestSquareIdx].position.z - 16,
      // };
      squares[oldestSquareIdx].position.set(destPoint.x, destPoint.y, destPoint.z);
      fpCtx.removeRigidBody(squares[oldestSquareIdx].userData.rigidBody);
      fpCtx.addTriMesh(squares[oldestSquareIdx]);
      squares[oldestSquareIdx].material = new THREE.MeshBasicMaterial({ color: 0xff0000 });
      squares[newestSquareIdx].material = new THREE.MeshBasicMaterial({ color: 0xadd8e6 });
      oldestSquareIdx = (oldestSquareIdx + 1) % squares.length;
      viz.fpCtx!.setSpawnPos(
        new THREE.Vector3(
          squares[oldestSquareIdx].position.x,
          squares[oldestSquareIdx].position.y + 10,
          squares[oldestSquareIdx].position.z
        ),
        new THREE.Vector3(viz.camera.rotation.x, viz.camera.rotation.y, viz.camera.rotation.z)
      );
      lastDir = curDir;
    }
  }, 250);
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const { checkpointMat, greenMosaic2Material, goldMaterial } = await buildMaterials(viz, loadedWorld);

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  initLevel(viz);

  initPylonsPostprocessing(viz, vizConf);
  function reset() {
    viz.fpCtx!.teleportPlayer(squares[oldestSquareIdx].position, locations.spawn.rot);
    viz.fpCtx!.reset();
    // this.curRunStartTimeSeconds = null;
    // if (this.winState?.displayComp) {
    //   unmount(this.winState.displayComp);
    // }
    //this.winState = null;
  }
  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
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
