import * as THREE from 'three';

import type { VizState } from '..';
import { initBaseScene } from '../util';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group) => {
  const engine = await import('../wasmComp/engine');
  await engine.default();

  const { ambientlight, light } = initBaseScene(viz);
  ambientlight.intensity = 1.8;
  viz.scene.fog = null;

  // Add in a white cube at the position of the light
  const lightCube = new THREE.Mesh(
    new THREE.BoxGeometry(10, 10, 10),
    new THREE.MeshBasicMaterial({ color: 0xffffff })
  );
  lightCube.position.copy(light.position);
  viz.scene.add(lightCube);

  const conduitStartPos = new THREE.Vector3(10, 40, 10);
  const conduitEndPos = new THREE.Vector3(100, 40, -100);
  const conduitRaduis = 5;

  // const conduitMesh = new THREE.Mesh(
  //   new THREE.TubeBufferGeometry(
  //     new THREE.LineCurve3(conduitStartPos, conduitEndPos),
  //     100,
  //     conduitRaduis,
  //     10,
  //     false
  //   ),
  //   new THREE.MeshStandardMaterial({ color: 0x181818, metalness: 0.5, roughness: 0.5 })
  // );
  // viz.scene.add(conduitMesh);

  // add cube at conduit start + end position
  // const conduitStartCube = new THREE.Mesh(
  //   new THREE.BoxGeometry(8, 8, 8),
  //   new THREE.MeshStandardMaterial({ color: 0x00ff00 })
  // );
  // conduitStartCube.position.copy(conduitStartPos);
  // viz.scene.add(conduitStartCube);
  // const conduitEndCube = new THREE.Mesh(
  //   new THREE.BoxBufferGeometry(8, 8, 8),
  //   new THREE.MeshStandardMaterial({ color: 0xff0000 })
  // );
  // conduitEndCube.position.copy(conduitEndPos);
  // viz.scene.add(conduitEndCube);

  const conduitParticles = new THREE.InstancedMesh(
    new THREE.BoxBufferGeometry(0.8, 0.8, 0.8),
    new THREE.MeshStandardMaterial({ color: 0x5311d6, metalness: 0.8, roughness: 1 }),
    80_000
  );
  conduitParticles.count;
  conduitParticles.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  viz.scene.add(conduitParticles);

  const conduitParticles2 = new THREE.InstancedMesh(
    new THREE.BoxBufferGeometry(0.8, 0.8, 0.8),
    new THREE.MeshStandardMaterial({ color: 0x8934eb, metalness: 0.8, roughness: 1 }),
    80_000
  );
  conduitParticles2.count;
  conduitParticles2.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  viz.scene.add(conduitParticles2);

  const conduitStatePtr = engine.create_conduit_particles_state(
    conduitStartPos.x,
    conduitStartPos.y,
    conduitStartPos.z,
    conduitEndPos.x,
    conduitEndPos.y,
    conduitEndPos.z,
    conduitRaduis
  );
  const conduitStatePtr2 = engine.create_conduit_particles_state(
    conduitStartPos.x,
    conduitStartPos.y,
    conduitStartPos.z,
    conduitEndPos.x,
    conduitEndPos.y,
    conduitEndPos.z,
    conduitRaduis * 2
  );
  viz.registerBeforeRenderCb((curTimeSecs, tDiffSecs) => {
    const newPositions = engine.tick_conduit_particles(conduitStatePtr, curTimeSecs, tDiffSecs);
    conduitParticles.count = newPositions.length / 3;

    const newPositions2 = engine.tick_conduit_particles(conduitStatePtr2, curTimeSecs, tDiffSecs);
    conduitParticles2.count = newPositions2.length / 3;

    const jitter = Math.pow((Math.sin(curTimeSecs) + 1) / 2, 1.5);

    const mat = new THREE.Matrix4();
    mat.makeScale(
      1 + Math.sin(curTimeSecs * 4) * 0.4,
      1 + Math.sin(curTimeSecs * 4) * 0.4,
      1 + Math.sin(curTimeSecs * 4) * 0.4
    );
    for (let i = 0; i < newPositions.length; i += 3) {
      mat.setPosition(
        newPositions[i] + Math.random() * jitter,
        newPositions[i + 1] + Math.random() * jitter,
        newPositions[i + 2] + Math.random() * jitter
      );
      conduitParticles.setMatrixAt(i / 3, mat);
    }

    mat.makeScale(
      0.6 + Math.sin(curTimeSecs * 2) * 0.2,
      0.6 + Math.sin(curTimeSecs * 2) * 0.2,
      0.6 + Math.sin(curTimeSecs * 2) * 0.2
    );
    for (let i = 0; i < newPositions.length; i += 3) {
      mat.setPosition(newPositions2[i], newPositions2[i + 1], newPositions2[i + 2]);
      conduitParticles2.setMatrixAt(i / 3, mat);
    }

    conduitParticles.instanceMatrix.needsUpdate = true;
    conduitParticles2.instanceMatrix.needsUpdate = true;

    conduitParticles.instanceMatrix.updateRange.offset = 0;
    conduitParticles.instanceMatrix.updateRange.count =
      (newPositions.length / 3) * conduitParticles.instanceMatrix.itemSize;
    conduitParticles2.instanceMatrix.updateRange.offset = 0;
    conduitParticles2.instanceMatrix.updateRange.count =
      (newPositions2.length / 3) * conduitParticles2.instanceMatrix.itemSize;
  });
};
