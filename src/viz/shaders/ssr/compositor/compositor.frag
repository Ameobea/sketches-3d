
#include <common>

uniform vec3 sceneCameraPos;
uniform float cameraNear;
uniform float cameraFar;
uniform mat4 cameraProjectionMatrix;
uniform mat4 cameraProjectionMatrixInv;
uniform mat4 cameraMatrixWorld;
uniform mat4 cameraMatrixWorldInverse;

uniform sampler2D ssrData;
uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;

varying vec2 vUv;

#define DITHERING
#include <dithering_pars_fragment>

float linearizeDepth(float depth, float zNear, float zFar) {
  float z_ndc = depth * 2. - 1.;
  return (2. * zNear * zFar) / (zFar + zNear - z_ndc * (zFar - zNear));
}

vec3 computeWorldPosFromDepth(float depth, vec2 coord) {
  float z = depth * 2. - 1.;
  vec4 clipSpacePosition = vec4(coord * 2. - 1., z, 1.);
  vec4 viewSpacePosition = cameraProjectionMatrixInv * clipSpacePosition;
  // Perspective division
  viewSpacePosition /= viewSpacePosition.w;
  vec4 worldSpacePosition = cameraMatrixWorld * viewSpacePosition;
  return worldSpacePosition.xyz;
}

vec3 computeReflectedRayDirection(vec3 fragPosWorldSpace, vec3 fragNormal) {
  // ray from camera -> fragment
  vec3 rayDirection = normalize(fragPosWorldSpace - sceneCameraPos);
  return reflect(rayDirection, fragNormal);
}

vec2 worldSpaceToScreenSpace(vec3 worldSpacePos) {
  vec4 viewSpacePos = cameraMatrixWorldInverse * vec4(worldSpacePos, 1.0);
  vec4 clipSpacePos = cameraProjectionMatrix * viewSpacePos;
  clipSpacePos /= clipSpacePos.w;
  return clipSpacePos.xy * 0.5 + 0.5;
}

vec3 raymarchReflection(vec3 startPos, vec3 direction) {
  float baseStepSizeWorldSpace = 0.5;
  float targetStepSizeScreenSpace = 0.5;

  float stepSize = baseStepSizeWorldSpace;
  uint maxSteps = 4000u;

  vec3 lastPos = startPos;
  vec3 curPos = startPos;
  bool hit = false;
  vec2 lastScreenPos = worldSpaceToScreenSpace(lastPos);
  for (uint i = 0u; i < maxSteps; i++) {
    curPos = lastPos + direction * stepSize;
    vec2 screenPos = worldSpaceToScreenSpace(curPos);

    // we want to try to take steps of a consistent size.
    float actualStepSizeScreenSpace = distance(screenPos, lastScreenPos);
    lastScreenPos = screenPos;
    float stepSizeScaleFactor = targetStepSizeScreenSpace / actualStepSizeScreenSpace;
    stepSize = mix(0.5, stepSize, stepSize * stepSizeScaleFactor);

    if (screenPos.x < 0.0 || screenPos.x > 1.0 || screenPos.y < 0.0 || screenPos.y > 1.0) {
      // return vec3(actualStepSizeScreenSpace);
      break;
    }

    float depth = texture2D(sceneDepth, screenPos).x;
    float linearDepth = linearizeDepth(depth, cameraNear, cameraFar);

    vec4 viewPos = cameraMatrixWorldInverse * vec4(curPos, 1.0);
    float expectedDepth = -viewPos.z;

    bool isBehind = expectedDepth > linearDepth + 0.01;
    if (isBehind) {
      hit = true;
      break;
    }

    lastPos = curPos;
  }

  if (!hit) {
    return vec3(-1.0);
  }

  // Binary search remains the same, using the updated depth calculations
  uint binarySearchSteps = 6u;
  vec3 bSearchStart = lastPos;
  vec3 bSearchEnd = curPos;
  for (uint i = 0u; i < binarySearchSteps; i++) {
    curPos = mix(bSearchStart, bSearchEnd, 0.5);
    vec2 screenPos = worldSpaceToScreenSpace(curPos);

    float depth = texture2D(sceneDepth, screenPos).x;
    float linearDepth = linearizeDepth(depth, cameraNear, cameraFar);

    vec4 viewPos = cameraMatrixWorldInverse * vec4(curPos, 1.0);
    float expectedDepth = -viewPos.z;

    bool isBehind = expectedDepth > linearDepth;
    if (isBehind) {
      bSearchEnd = curPos;
    } else {
      bSearchStart = curPos;
    }
  }

  return texture2D(sceneDiffuse, worldSpaceToScreenSpace(curPos)).rgb;
}

void main() {
  vec4 diffuse = texture2D(sceneDiffuse, vUv);
  vec4 data = texture2D(ssrData, vUv);

  float reflectionAlpha = data.a;
  // use base scene color if this fragment isn't reflective
  if (reflectionAlpha == 0. || reflectionAlpha == 1.) {
    gl_FragColor = diffuse;
    return;
  }

  // this normal is in world space
  vec3 surfaceNormal = data.rgb;

  float rawDepth = texture2D(sceneDepth, vUv).x;
  vec3 fragPosWorldSpace = computeWorldPosFromDepth(rawDepth, vUv);
  vec3 reflectedRayDirection = computeReflectedRayDirection(fragPosWorldSpace, surfaceNormal);

  vec3 reflectedColor = raymarchReflection(fragPosWorldSpace, reflectedRayDirection);
  if (reflectedColor.x < 0.) {
    gl_FragColor = diffuse;
    return;
  }

  gl_FragColor = vec4(mix(diffuse.rgb, reflectedColor, reflectionAlpha), 1.);

  #include <dithering_fragment>
}
