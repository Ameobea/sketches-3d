// Swept tube: a closed circular profile along a gently humped spine. rail_sweep emits
// analytic UVs (U = arc length along the spine, V = around the ring), so a texture maps
// along and around the tube following its surface instead of a triplanar world projection.
// `split_seams=true` makes the V wrap seamless (no smeared seam column) and gives the caps
// planar UVs + a real tangent frame (so the normal map works there too); this trades away
// watertight 2-manifold topology, which is fine for a render-only mesh.
// `cap_uv_scale` undoes the material's anisotropic uvScale ([0.1, 1]) on the caps so they read
// isotropically at the body's ~0.1/world-unit density instead of stretched 10x in V.
export mesh = rail_sweep(
  spine_resolution=80,
  ring_resolution=36,
  spine=|u| v3(u * 40 - 20, sin(u * pi) * 6, 0),
  profile=|u, v| v2(cos(v * 2 * pi) * 1.5, sin(v * 2 * pi) * 1.5),
  frame_mode='rmf',
  capped=true,
  split_seams=true,
  cap_uv_scale=v2(1, 0.1)
)
