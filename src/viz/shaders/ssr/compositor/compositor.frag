
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
  vec4 viewSpacePos = cameraMatrixWorldInverse * vec4(worldSpacePos, 1.);
  vec4 clipSpacePos = cameraProjectionMatrix * viewSpacePos;
  clipSpacePos /= clipSpacePos.w;
  return clipSpacePos.xy * 0.5 + 0.5;
}

vec4 raymarchReflection(vec3 startPos, vec3 direction) {
  float baseStepSizeWorldSpace = 1.1;
  float targetStepSizeScreenSpace = 1. / 500.;
  float maxRayLength = cameraFar - cameraNear;

  float stepSizeWorldSpace = baseStepSizeWorldSpace;
  uint maxSteps = 1350u;
  float minStepSizeWorldSpace = 0.01;
  float maxStepSizeWorldSpace = maxRayLength / 200.;

  // TODO: It might be interesting to have a mipmapped depth texture to read from.
  //       That way, big regions can be skipped if they contain nothing remotely close.

  vec3 lastPos = startPos;
  vec3 curPos;
  bool hit = false;
  vec2 lastScreenPos = worldSpaceToScreenSpace(startPos);
  for (uint i = 0u; i < maxSteps; i++) {
    curPos = lastPos + direction * stepSizeWorldSpace;
    float traveledDistance = distance(startPos, curPos);
    if (traveledDistance > maxRayLength) {
      break;
    }
    vec2 screenPos = worldSpaceToScreenSpace(curPos);

    // we want to try to take steps of a consistent size.
    float actualStepSizeScreenSpace = distance(screenPos, lastScreenPos);
    if (actualStepSizeScreenSpace < 0.00001) {
      return vec4(-1.);
    }
    float stepSizeScaleFactor = targetStepSizeScreenSpace / actualStepSizeScreenSpace;
    stepSizeWorldSpace = mix(stepSizeWorldSpace, stepSizeWorldSpace * stepSizeScaleFactor, 0.5);
    stepSizeWorldSpace = clamp(stepSizeWorldSpace, minStepSizeWorldSpace, maxStepSizeWorldSpace);

    // TODO: Need to handle stepping back to the edge to avoid jagged edges due to step size
    if (screenPos.x < 0.0 || screenPos.x > 1.0 || screenPos.y < 0.0 || screenPos.y > 1.0) {
      break;
    }

    float rawSceneDepth = texture2D(sceneDepth, screenPos).x;
    if (rawSceneDepth == 1.) {
      lastPos = curPos;
      lastScreenPos = screenPos;
      continue;
    }
    float linearSceneDepth = linearizeDepth(rawSceneDepth, cameraNear, cameraFar);

    vec4 viewPos = cameraMatrixWorldInverse * vec4(curPos, 1.0);
    float rayDepth = -viewPos.z;

    float depthDiff = rayDepth - linearSceneDepth;
    float rayDistanceThisStepWorldSpace = distance(lastPos, curPos);

    bool isBehind = depthDiff > 0. && depthDiff < rayDistanceThisStepWorldSpace * 2.;
    if (isBehind) {
      hit = true;
      break;
    }

    lastPos = curPos;
    lastScreenPos = screenPos;
  }

  if (!hit) {
    return vec4(-1.0);
  }

  vec2 reflectedColorScreenCoord = worldSpaceToScreenSpace(curPos);
  vec3 reflectedColor = texture2D(sceneDiffuse, reflectedColorScreenCoord).rgb;

  // fade out reflections from the edges of the screen to help hide discontinuities
  vec2 minDistanceToEdges = min(reflectedColorScreenCoord.xy, 1. - reflectedColorScreenCoord.xy);
  float minDistanceToAnyEdge = min(minDistanceToEdges.x, minDistanceToEdges.y);
  float alpha = smoothstep(0., 0.07, minDistanceToAnyEdge);

  // return vec4(reflectedColorScreenCoord, 0., 1.);
  return vec4(reflectedColor, alpha);
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

  vec4 reflectedColor = raymarchReflection(fragPosWorldSpace, reflectedRayDirection);
  if (reflectedColor.x < 0.) {
    gl_FragColor = diffuse;
    return;
  }

  gl_FragColor = vec4(mix(diffuse.rgb, reflectedColor.rgb, reflectionAlpha * reflectedColor.a), 1.);

  #include <dithering_fragment>
}
