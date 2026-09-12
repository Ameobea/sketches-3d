import assert from 'node:assert/strict';
import { test } from 'node:test';
import { buildGeodesicPathBuffers } from './geodesicPathBuffers';

test('geodesic graph never refers beyond its absolute coordinate buffer', () => {
  for (const values of [[], [0, 1000], [1, 2, -3, 4, 5, -6]]) {
    const { positions, indices } = buildGeodesicPathBuffers(new Float32Array(values));
    assert.ok(indices.every(i => i < positions.length / 2));
    assert.equal(positions.length, values.length + 2);
    assert.equal(indices.length, Math.max(1, values.length / 2) * 3);
  }
  const one = buildGeodesicPathBuffers(new Float32Array([0, 1000]));
  assert.deepEqual([...one.positions], [0, 0, 0, 1000]);
  assert.deepEqual([...one.indices], [0, 1, 1]);
  const several = buildGeodesicPathBuffers(new Float32Array([1, 2, -3, 4, 5, -6]));
  assert.deepEqual([...several.positions], [0, 0, 1, 2, -2, 6, 3, 0]);
});
