varying vec2 vUv;

uniform float cameraNear;
uniform float cameraFar;

uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDistance;

const vec4 FOG_COLOR = vec4(0.15, 0.15, 0.22, 1.);

const vec3[2] FOG_COLOR_RAMP = vec3[2](
  vec3(0.15, 0.15, 0.22),
  vec3(0.11, 0.11, 0.14)
);

#include <packing>

float getFogFactor(float dist) {
  // linear
  // return 1. - (dist - cameraNear) / (cameraFar - cameraNear);
  // exponential: log(60*dist)/10
  float factor = (log(60. * dist) / 10. - 0.4) * 2.;
  return clamp(factor, 0., 1.);
}

void main() {
  float unpackedDistance = unpackRGBAToDepth(texture2D(sceneDistance, vUv));
  vec4 diffuse = texture2D(sceneDiffuse, vUv);

  if (unpackedDistance > 0.99) {
    gl_FragColor = vec4(mix(diffuse.rgb, FOG_COLOR_RAMP[1].rgb, 0.55), 1.);
    return;
  }

  float frustumLength = cameraFar - cameraNear;
  float dist = frustumLength * unpackedDistance + cameraNear;

  float fogFactor = getFogFactor(dist);
  // gl_FragColor = vec4(fogFactor, fogFactor, fogFactor, 1.);
  vec3 outColor = mix(diffuse.rgb, FOG_COLOR_RAMP[1].rgb, fogFactor * 0.955);
  gl_FragColor = vec4(outColor, 1.);
}
