/*
 * Adapted from: https://github.com/Ameobea/three-good-godrays/blob/main/src/compositor.frag
 *
 * Which itself was adapted from this demo: https://n8python.github.io/goodGodRays/
 * By: https://github.com/n8python
 *
 * Composites low-resolution volumetric fog into the full-resolution scene using
 * depth-aware upsampling to preserve sharp edges at depth discontinuities.
 *
 * For each full-res pixel, we sample a neighborhood of low-res fog texels
 * and weight each by spatial proximity and depth similarity, preventing
 * fog from bleeding across depth edges.
 */

#include <common>

// The output of the volumetric pass (rgb = fog color, a = density)
uniform sampler2D fogTexture;
uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;
uniform vec2 resolution;
uniform vec2 fogResolution;
uniform float near;
uniform float far;
varying vec2 vUv;

#define DITHERING
#include <dithering_pars_fragment>

float linearize_depth(float d, float zNear, float zFar) {
  return zNear * zFar / (zFar + d * (zNear - zFar));
}

void main() {
  float rawDepth = texture2D(sceneDepth, vUv).x;
  float correctDepth = linearize_depth(rawDepth, near, far);

  vec2 texelSize = 1.0 / fogResolution;

  // Position of this full-res pixel in low-res texel coordinates.
  // Texel (i,j) has its center at UV ((i + 0.5) / width, (j + 0.5) / height),
  // so the -0.5 converts from UV-scaled to integer texel space.
  vec2 texelPos = vUv * fogResolution - 0.5;
  vec2 base = floor(texelPos);
  vec2 f = texelPos - base;

  float totalWeight = 0.0;
  vec3 totalColor = vec3(0.0);
  float totalDensity = 0.0;

  // Joint bilateral upsampling: sample low-res texels in a neighborhood,
  // weighting each by spatial proximity and depth similarity.
  // JBU_EXTENT=0 gives the 2x2 bilinear quad (4 taps),
  // JBU_EXTENT=1 extends one texel in each direction (4x4 = 16 taps).
  for (int y = -JBU_EXTENT; y <= 1 + JBU_EXTENT; y++) {
    for (int x = -JBU_EXTENT; x <= 1 + JBU_EXTENT; x++) {
      vec2 sampleUv = (base + vec2(float(x), float(y)) + 0.5) * texelSize;
      vec4 data = texture2D(fogTexture, sampleUv);

      // Sample scene depth at the low-res texel center and linearize it
      float sampleDepth = texture2D(sceneDepth, sampleUv).x;
      sampleDepth = linearize_depth(sampleDepth, near, far);

      // Gaussian spatial weight based on distance in texel units
      vec2 d = vec2(float(x), float(y)) - f;
      float spatialW = exp(-dot(d, d) / (2.0 * JBU_SPATIAL_SIGMA * JBU_SPATIAL_SIGMA));

      // Gaussian depth weight based on relative depth difference
      float depthDiff = (sampleDepth - correctDepth) / max(correctDepth, 0.001);
      float depthW = exp(-0.5 * depthDiff * depthDiff / (JBU_DEPTH_SIGMA * JBU_DEPTH_SIGMA));

      float w = spatialW * depthW;
      totalWeight += w;
      totalColor += data.rgb * w;
      totalDensity += data.a * w;
    }
  }

  vec3 fogColor = totalWeight > 0.0 ? totalColor / totalWeight : vec3(0.0);
  float fogDensity = totalWeight > 0.0 ? totalDensity / totalWeight : 0.0;

  vec3 sceneColor = texture2D(sceneDiffuse, vUv).rgb;
  gl_FragColor = vec4(mix(sceneColor, fogColor, fogDensity), 1.0);

  #include <dithering_fragment>
}
