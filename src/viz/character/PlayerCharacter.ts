import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { LevelLoadHandle } from 'src/viz/levelDef/loadLevelDef';
import { getPlayerColliderCenterToFeetOffset } from 'src/viz/physicsConfig';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { ProceduralLegs, type ProceduralLegsConf } from './ProceduralLegs';
import { buildSkeleton } from './skeleton';
import { skinRigid } from './skinning';

export interface PlayerCharacterOpts {
  /**
   * `camera`: the body faces where the camera looks and strafes.  `velocity`: the body turns
   * to face the movement direction.  Default `camera`.
   */
  facing?: 'camera' | 'velocity';
  gait?: Partial<ProceduralLegsConf>;
}

/** Model units; tuned for a body about 8 units tall with 4-unit legs. */
export const DefaultGait: ProceduralLegsConf = {
  stride: 3.5,
  lift: 0.6,
  duty: 0.7,
  reach: 0.95,
  idleCrouch: 0.15,
  minCycleSeconds: 0.12,
  maxCycleSeconds: 1,
  tuck: 3,
  airBend: 0.1,
  jumpReleaseTau: 0.09,
  settleTau: 0.17,
  crouchInTau: 0.04,
  crouchOutTau: 0.17,
  landCrouch: 1,
  landAttackTau: 0.03,
  landRecoverTau: 0.08,
  minWalkSpeed: 0.5,
  wallCadence: 0.3,
};

const YawTau = 0.03;

const expDecay = (dt: number, tau: number) => 1 - Math.exp(-dt / tau);
const wrapAngle = (a: number) => Math.atan2(Math.sin(a), Math.cos(a));

/**
 * Builds the player's skinned character from the level def's `character` asset and drives it:
 * fit to the collider, yaw, first-person hiding, and the procedural legs fed by physics.  Sets
 * `sceneConf.player.mesh`, which physics keeps positioned.  Requires physics to exist.
 */
export const setupPlayerCharacter = async (viz: Viz, handle: LevelLoadHandle, opts: PlayerCharacterOpts) => {
  const fpCtx = viz.fpCtx;
  if (!fpCtx || !handle.character) {
    return;
  }
  const asset = await handle.character;
  if (viz.destroyed) {
    return;
  }
  const gait = { ...DefaultGait, ...opts.gait };
  const facing = opts.facing ?? 'camera';

  const root = new THREE.Group();
  // the level's forward is +Z of the root; the asset faces -X, so the pivot re-aims it
  const pivot = new THREE.Group();
  pivot.rotation.y = Math.PI / 2;
  root.add(pivot);
  const model = new THREE.Group();

  const skeleton = buildSkeleton(asset.chains);
  model.add(skeleton.root);
  model.updateMatrixWorld(true);
  const threeSkeleton = new THREE.Skeleton(skeleton.bones);

  const placeholder = buildCustomShader(
    { color: new THREE.Color(0xaaadb3), metalness: 0.75, roughness: 0.45 },
    {},
    { noOcclusion: true }
  );
  const bbox = new THREE.Box3();
  const skinned = asset.meshes.map(({ mesh, materialName }) => {
    mesh.updateMatrix();
    const geometry = mesh.geometry.clone().applyMatrix4(mesh.matrix);
    geometry.computeBoundingBox();
    bbox.union(geometry.boundingBox!);
    skinRigid(geometry, skeleton);
    const sm = new THREE.SkinnedMesh<THREE.BufferGeometry, THREE.Material>(geometry, placeholder);
    sm.castShadow = false;
    sm.receiveShadow = true;
    sm.frustumCulled = false;
    sm.userData.occlusionExclude = true;
    model.add(sm);
    sm.updateMatrixWorld(true);
    sm.bind(threeSkeleton);
    return { sm, materialName };
  });

  const shape = fpCtx.playerColliderShape;
  const h = fpCtx.playerColliderHeight;
  const r = fpCtx.playerColliderRadius;
  const colliderHeight = shape === 'capsule' ? h + 2 * r : shape === 'cylinder' ? h : 2 * r;
  const scale = colliderHeight / (bbox.max.y - skeleton.floorY);
  const legs = new ProceduralLegs(skeleton, pivot, gait);
  pivot.scale.setScalar(scale);
  pivot.position.y = -getPlayerColliderCenterToFeetOffset(shape, h, r) - skeleton.floorY * scale;
  pivot.add(model);

  handle.complete.then(() => {
    for (const { sm, materialName } of skinned) {
      const mat = materialName ? handle.builtMaterials.get(materialName) : undefined;
      if (mat) {
        sm.material = mat;
      }
    }
  });

  viz.sceneConf.player ??= {};
  viz.sceneConf.player.mesh = root;
  viz.scene.add(root);
  console.info(
    `[character] ${asset.meshes.length} mesh(es), ${skeleton.bones.length} joints, ${skeleton.legs.length} legs, fit scale ${scale.toFixed(3)}`
  );

  const prevPos = new THREE.Vector3();
  const bodyVel = new THREE.Vector3();
  let hasPrevPos = false;
  let jumpQueued = false;
  let landQueued = false;
  let yaw: number | null = null;

  fpCtx.registerJumpCb(() => {
    jumpQueued = true;
  });
  fpCtx.registerLandCb(() => {
    landQueued = true;
  });
  fpCtx.registerTeleportCb(() => {
    legs.reset();
    hasPrevPos = false;
  });
  viz.registerOnRespawnCb(() => {
    yaw = null;
  });

  viz.registerBeforeRenderCb((_curTimeSeconds, dt) => {
    const isFirstPerson = viz.cameraController?.isFirstPerson ?? false;
    root.visible = !isFirstPerson;
    if (isFirstPerson) {
      hasPrevPos = false;
      jumpQueued = false;
      landQueued = false;
      legs.reset();
      return;
    }

    const [wx, , wz] = fpCtx.playerStateGetters.getWalkVelocity();
    const speed = Math.hypot(wx, wz);
    const onGround = fpCtx.playerStateGetters.getIsOnGround();
    const cameraYaw = (viz.cameraController?.angles.theta ?? 0) + Math.PI;

    if (yaw === null) {
      yaw = cameraYaw;
    }
    const targetYaw = facing === 'camera' ? cameraYaw : speed > gait.minWalkSpeed ? Math.atan2(wx, wz) : null;
    if (targetYaw !== null) {
      yaw += wrapAngle(targetYaw - yaw) * expDecay(dt, YawTau);
    }
    root.rotation.y = yaw;

    // body velocity from the collider's actual motion; warps reset the history via the teleport cb
    if (hasPrevPos && dt > 0) {
      bodyVel.subVectors(root.position, prevPos).divideScalar(dt);
    } else {
      bodyVel.set(0, 0, 0);
    }
    prevPos.copy(root.position);
    hasPrevPos = true;

    legs.update({
      dt,
      bodyVel,
      walkSpeed: speed,
      onGround,
      jumped: jumpQueued,
      landed: landQueued,
      probeGround: fpCtx.rayTestDown,
    });
    jumpQueued = false;
    landQueued = false;
  }, 2);
};
