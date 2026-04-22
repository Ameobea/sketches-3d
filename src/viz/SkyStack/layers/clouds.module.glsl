// Shared fBm sampler for all cloud instances. `MAX_HAZE_OCTAVES` is a compile-
// time #define aggregated (max) across every cloud layer's `octaves` config,
// so the loop bound is a literal the driver can unroll.
float skyFbm(vec3 x, int octaves, float lacunarity, float gain) {
  float v = 0.0;
  float a = 1.0;
  float norm = 0.0;
  vec3 shift = vec3(100.0);
  for (int i = 0; i < MAX_HAZE_OCTAVES; i++) {
    if (i >= octaves) {
      break;
    }
    v += a * noise(x);
    norm += a;
    x = x * lacunarity + shift;
    a *= gain;
  }
  return v / max(norm, 1e-6);
}
