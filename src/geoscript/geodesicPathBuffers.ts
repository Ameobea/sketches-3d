/** A chain of 2D movements, encoded as absolute points and degenerate graph triangles. */
export const buildGeodesicPathBuffers = (movements: Float32Array) => {
  if (movements.length % 2) throw new Error('Geodesic movements must be pairs');
  const count = movements.length / 2;
  const positions = new Float32Array((count + 1) * 2);
  const indices = new Uint32Array(Math.max(1, count) * 3);
  for (let i = 0; i < count; i++) {
    positions[(i + 1) * 2] = positions[i * 2] + movements[i * 2];
    positions[(i + 1) * 2 + 1] = positions[i * 2 + 1] + movements[i * 2 + 1];
    indices.set([i, i + 1, i + 1], i * 3);
  }
  // An empty movement list still supplies the start vertex to the C++ graph walker.
  return { positions, indices };
};
