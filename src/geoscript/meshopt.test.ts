import assert from 'node:assert/strict';
import { test } from 'node:test';
import { initMeshopt, meshopt_simplify } from './meshopt';

test('vertex protection works with geometry-only input and retains source buffers', async () => {
  await initMeshopt();
  const points: number[] = [];
  const triangles: number[] = [];
  for (let y = 0; y < 5; y++) for (let x = 0; x < 5; x++) points.push(x, y, 0);
  for (let y = 0; y < 4; y++)
    for (let x = 0; x < 4; x++) {
      const a = y * 5 + x;
      triangles.push(a, a + 1, a + 5, a + 1, a + 6, a + 5);
    }
  const positions = new Float32Array(points),
    indices = new Uint32Array(triangles);
  const empty = new Float32Array();
  const locked = meshopt_simplify(indices, positions, empty, empty, new Uint8Array(25).fill(1), 10);
  assert.equal(locked.length, indices.length);
  const reduced = meshopt_simplify(indices, positions, empty, empty, new Uint8Array(25), 10);
  assert.ok(reduced.length < indices.length);
  assert.ok(reduced.every(i => i < 25));
  assert.deepEqual([...positions], points);
  assert.deepEqual([...indices], triangles);
});
