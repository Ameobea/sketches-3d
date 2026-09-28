import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import * as THREE from 'three';

import type { AmmoInterface } from '../ammojs/ammoTypes';
import { SharedTrimeshCache, buildCollisionShapeFromMesh } from './collisionShapes';

const loadAmmo = async (): Promise<AmmoInterface> => {
  const { Ammo } = (await import('../ammojs/ammo.wasm.js')) as any;
  const wasmBinary = readFileSync(new URL('../ammojs/ammo.wasm.wasm', import.meta.url));
  return Ammo.apply({}, [{ wasmBinary }]);
};

/** Creased terrain: plenty of convex/concave internal edges for the edge-info map to matter. */
const buildTerrain = () => {
  const n = 40;
  const pos: number[] = [];
  const idx: number[] = [];
  for (let z = 0; z <= n; z++) {
    for (let x = 0; x <= n; x++) {
      const u = x / 2 - 10;
      const v = z / 2 - 10;
      pos.push(u, Math.sin(u * 0.9) * 0.6 + Math.abs(Math.cos(v * 0.7)) * 0.8, v);
    }
  }
  for (let z = 0; z < n; z++) {
    for (let x = 0; x < n; x++) {
      const a = z * (n + 1) + x;
      idx.push(a, a + n + 1, a + 1, a + 1, a + n + 1, a + n + 2);
    }
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(pos), 3));
  geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(idx), 1));
  return geometry;
};

const makeWorld = (Ammo: AmmoInterface) => {
  const A = Ammo as any;
  const config = new A.btDefaultCollisionConfiguration();
  const world = new A.btDiscreteDynamicsWorld(
    new A.btCollisionDispatcher(config),
    new A.btDbvtBroadphase(),
    new A.btSequentialImpulseConstraintSolver(),
    config
  );
  world.getBroadphase().getOverlappingPairCache().setInternalGhostPairCallback(new A.btGhostPairCallback());
  const ghost = new A.btPairCachingGhostObject();
  const capsule = new A.btCapsuleShape(0.5, 1.2);
  ghost.setCollisionShape(capsule);
  ghost.setCollisionFlags(16);
  const controller = new A.btKinematicCharacterController(ghost, capsule, 0.4, new A.btVector3(0, 1, 0));
  controller.setGravity(new A.btVector3(0, -30, 0));
  // The constructor leaves these uninitialized; production always sets them.
  controller.setMoveSpeed(12, 12);
  controller.setJumpSpeed(15);
  controller.setMinJumpDelay(0.25);
  controller.setCoyoteTime(0);
  controller.setBoostArmLeniency(0);
  controller.setDashConfig(false, 0, 0, false);
  controller.setExternalVelocity(new A.btVector3(0, 0, 0));
  world.addCollisionObject(ghost, 32, 1 | 2);
  world.addAction(controller);
  return { world, controller };
};

const addStaticBody = (Ammo: AmmoInterface, world: any, shape: any, mesh: THREE.Mesh) => {
  const A = Ammo as any;
  const transform = new A.btTransform();
  transform.setIdentity();
  transform.setOrigin(new A.btVector3(mesh.position.x, mesh.position.y, mesh.position.z));
  const q = mesh.quaternion;
  transform.setRotation(new A.btQuaternion(q.x, q.y, q.z, q.w));
  const info = new A.btRigidBodyConstructionInfo(
    0,
    new A.btDefaultMotionState(transform),
    shape,
    new A.btVector3(0, 0, 0)
  );
  const body = new A.btRigidBody(info);
  body.setCollisionFlags(1);
  world.addRigidBody(body);
  return body;
};

const placed = (geometry: THREE.BufferGeometry, scale: [number, number, number]) => {
  const mesh = new THREE.Mesh(geometry);
  mesh.position.set(3, -2, 1);
  mesh.quaternion.setFromEuler(new THREE.Euler(0, 0.3, 0));
  mesh.scale.set(...scale);
  return mesh;
};

test('shared + uniformly rescaled trimesh behaves like a dedicated build', async () => {
  const Ammo = await loadAmmo();
  const btvec3 = (x: number, y: number, z: number) => new (Ammo as any).btVector3(x, y, z);
  const geometry = buildTerrain();
  const target: [number, number, number] = [6, 4.5, 6];

  const dedicated = makeWorld(Ammo);
  const dedicatedShape = buildCollisionShapeFromMesh(Ammo, btvec3, placed(geometry, target)).shape;
  addStaticBody(Ammo, dedicated.world, dedicatedShape, placed(geometry, target));

  const shared = makeWorld(Ammo);
  const cache = new SharedTrimeshCache(Ammo, btvec3);
  buildCollisionShapeFromMesh(Ammo, btvec3, placed(geometry, [2, 1.5, 2]), undefined, undefined, cache);
  const sharedShape = buildCollisionShapeFromMesh(
    Ammo,
    btvec3,
    placed(geometry, target),
    undefined,
    undefined,
    cache
  ).shape;
  assert.equal(
    (sharedShape as any).getChildShape !== undefined,
    true,
    'target instance should be a scaled wrapper'
  );
  addStaticBody(Ammo, shared.world, sharedShape, placed(geometry, target));

  const rng = (() => {
    let s = 12345;
    return () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
  })();
  let rayHits = 0;
  for (let i = 0; i < 400; i++) {
    const from = [rng() * 50, 10 + rng() * 5, rng() * 50];
    const to = [from[0] + rng() * 30, -15, from[2] + rng() * 30];
    const fa = dedicated.controller.cameraRayTest(dedicated.world, ...from, ...to);
    const fb = shared.controller.cameraRayTest(shared.world, ...from, ...to);
    assert.ok(Math.abs(fa - fb) < 1e-4, `ray ${i}: fraction ${fa} vs ${fb}`);
    if (fa < 1) {
      rayHits++;
      const na = [
        dedicated.controller.getCameraRayHitNormalX(),
        dedicated.controller.getCameraRayHitNormalY(),
        dedicated.controller.getCameraRayHitNormalZ(),
      ];
      const nb = [
        shared.controller.getCameraRayHitNormalX(),
        shared.controller.getCameraRayHitNormalY(),
        shared.controller.getCameraRayHitNormalZ(),
      ];
      assert.ok(na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2] > 0.99999, `ray ${i}: normal ${na} vs ${nb}`);
    }
    const sa = dedicated.controller.cameraSphereSweep(dedicated.world, ...from, ...to, 0.7, false);
    const sb = shared.controller.cameraSphereSweep(shared.world, ...from, ...to, 0.7, false);
    assert.ok(Math.abs(sa - sb) < 1e-4, `sweep ${i}: fraction ${sa} vs ${sb}`);
  }
  assert.ok(rayHits > 300, `expected most rays to hit, got ${rayHits}`);

  // Walk the character across creases in both worlds: contacts, step-up, and internal-edge fixup.
  let maxDrift = 0;
  let groundedSteps = 0;
  for (const { controller } of [dedicated, shared]) {
    controller.warp(new (Ammo as any).btVector3(-20, 12, -15));
  }
  for (let step = 0; step < 900; step++) {
    const theta = Math.PI / 4 + Math.sin(step / 90) * 0.8;
    for (const { world, controller } of [dedicated, shared]) {
      controller.setInputState(1, theta, 0, true);
      world.substepSimulation(1 / 160);
    }
    const pa = dedicated.controller.getPosition();
    const pb = shared.controller.getPosition();
    maxDrift = Math.max(maxDrift, Math.hypot(pa.x() - pb.x(), pa.y() - pb.y(), pa.z() - pb.z()));
    assert.equal(
      dedicated.controller.onGround(),
      shared.controller.onGround(),
      `step ${step}: onGround diverged`
    );
    groundedSteps += dedicated.controller.onGround() ? 1 : 0;
  }
  assert.ok(groundedSteps > 300, `character should mostly be walking on the terrain (${groundedSteps})`);
  assert.ok(maxDrift < 1e-3, `character trajectories diverged by ${maxDrift}`);
});

test('shared trimesh lifetimes: freed exactly once, only after the last body releases it', async () => {
  const Ammo = await loadAmmo();
  const btvec3 = (x: number, y: number, z: number) => new (Ammo as any).btVector3(x, y, z);
  const destroyed: number[] = [];
  const realDestroy = Ammo.destroy.bind(Ammo);
  (Ammo as any).destroy = (obj: any) => {
    destroyed.push(Ammo.getPointer(obj));
    realDestroy(obj);
  };
  const destroyCount = (ptr: number) => destroyed.filter(p => p === ptr).length;

  const cache = new SharedTrimeshCache(Ammo, btvec3);
  const geometry = buildTerrain();
  const acquire = (scale: [number, number, number]) =>
    buildCollisionShapeFromMesh(Ammo, btvec3, placed(geometry, scale), undefined, undefined, cache);
  const bvhOf = (r: { shape: any }) =>
    Ammo.getPointer(r.shape.getChildShape ? r.shape.getChildShape() : r.shape);

  const a = acquire([2, 2, 2]);
  const b = acquire([2, 2, 2]);
  const c = acquire([5, 5, 5]);
  const mirrored = acquire([-2, 2, 2]);
  const base = bvhOf(a);
  assert.equal(bvhOf(b), base, 'identical scale shares the BVH directly');
  assert.equal(bvhOf(c), base, 'uniform multiple shares the BVH through a scaled wrapper');
  assert.notEqual(bvhOf(mirrored), base, 'mirrored placement needs its own flipped-winding BVH');
  assert.equal(Ammo.getPointer(a.shape), base);
  assert.notEqual(Ammo.getPointer(c.shape), base);

  b.destroyShape!(Ammo);
  b.destroyShape!(Ammo);
  a.destroyShape!(Ammo);
  assert.equal(destroyCount(base), 0, 'still referenced by the scaled instance');
  assert.ok(cache.isShared(c.shape as any) === false && cache.isShared((c.shape as any).getChildShape()));
  const wrapper = Ammo.getPointer(c.shape);
  c.destroyShape!(Ammo);
  assert.equal(destroyCount(wrapper), 1, 'per-body wrapper destroyed with its body');
  assert.equal(destroyCount(base), 1, 'BVH freed when the last reference goes');
  c.destroyShape!(Ammo);
  assert.equal(destroyCount(base), 1, 'repeat release is a no-op');

  const rebuilt = acquire([2, 2, 2]);
  assert.ok(cache.isShared(rebuilt.shape as any), 'a later acquire rebuilds instead of reusing freed memory');

  // Mutating the geometry must not hand out a stale BVH, and must not free the one still in use.
  const oldVersion = bvhOf(rebuilt);
  geometry.attributes.position.needsUpdate = true;
  const fresh = acquire([2, 2, 2]);
  assert.ok(cache.isShared(fresh.shape as any));
  rebuilt.destroyShape!(Ammo);
  assert.equal(destroyCount(oldVersion), 1);
  assert.ok(cache.isShared(fresh.shape as any), 'new version unaffected by releasing the old one');

  // Teardown with bodies never removed: everything is freed once, later releases stay no-ops.
  const mirroredBvh = bvhOf(mirrored);
  const freshBvh = bvhOf(fresh);
  assert.equal(cache.dispose(), 2);
  assert.equal(destroyCount(mirroredBvh), 1);
  assert.equal(destroyCount(freshBvh), 1);
  mirrored.destroyShape!(Ammo);
  fresh.destroyShape!(Ammo);
  assert.equal(destroyCount(mirroredBvh), 1);
  assert.equal(destroyCount(freshBvh), 1);
});
