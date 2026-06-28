import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const playerHeight = 3.5;
  const playerRadius = 1;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader({
      color: new THREE.Color(0x8d3d9f),
      metalness: 0.18,
      roughness: 0.82,
    })
  );
  playerMesh.castShadow = false;
  playerMesh.receiveShadow = true;

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: Infinity,
    [Score.S]: Infinity,
    [Score.A]: Infinity,
    [Score.B]: Infinity,
  };
  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    {
      spawn: {
        pos: new THREE.Vector3(0, 3, 8),
        rot: new THREE.Vector3(0, Math.PI, 0),
      },
    },
    scoreThresholds,
    { checkpoint: new THREE.MeshStandardMaterial({ color: 0x80f0ff, emissive: 0x173845, roughness: 0.32 }) },
    'jump_pad_speedup_test',
    true,
    {
      gravity: 220,
      gravityShaping: {
        riseMultiplier: 1.0,
        apexMultiplier: 0.6,
        fallMultiplier: 1.2,
        apexThreshold: 4.0,
        kneeWidth: 0.1,
      },
      player: {
        playerColliderShape: 'capsule',
        mesh: playerMesh,
        colliderSize: { height: playerHeight, radius: playerRadius },
        playerShadow: { radius: playerRadius, intensity: 0.85 },
        moveSpeed: { onGround: 12.5, inAir: 18 },
        jumpVelocity: 76,
        terminalVelocity: 180,
        dashConfig: {
          chargeConfig: { curCharges: rwritable(Infinity) },
          dashMagnitude: 16,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.135,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.3,
      },
      viewMode: {
        type: 'thirdPerson',
        distance: 15,
        cameraFOV: 75,
        zoomEnabled: true,
        maxZoomDistance: 50,
      },
    }
  );

  initPylonsPostprocessing(viz, vizConf, false, { toneMapping: { mode: 'agx', exposure: 0.9 } });

  const handle = viz.levelLoadHandle;
  if (handle) {
    handle.setSceneRuntime(pkManager.runtime, 'scene_def_test');
  }

  return pkManager.buildSceneConfig();
};
