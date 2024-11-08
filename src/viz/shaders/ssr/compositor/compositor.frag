
#include <common>

uniform sampler2D ssrData;
uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;

varying vec2 vUv;

#define DITHERING
#include <dithering_pars_fragment>

float linearize_depth(float d, float zNear, float zFar) {
  return zNear * zFar / (zFar + d * (zNear - zFar));
}

void main() {
  vec4 diffuse = texture2D(sceneDiffuse, vUv);
  vec4 data = texture2D(ssrData, vUv);

  float reflectionAlpha = data.a;
  if (reflectionAlpha == 0.0) {
    gl_FragColor = diffuse;
    return;
  }

  vec3 reflectedColor = data.rgb;

  float rawDepth = texture2D(sceneDepth, vUv).x;
  float correctDepth = linearize_depth(rawDepth, 0.1, 1000.0);

  // TODO: real impl

  gl_FragColor = vec4(mix(reflectedColor, diffuse.rgb, reflectionAlpha), 1.0);

  #include <dithering_fragment>
}
