// #include <common>

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

// #define DITHERING
// #include <dithering_pars_fragment>

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

vec3 binarySearchHit(vec3 startPosWorldSpace_, vec3 endPosWorldSpace_) {
  vec3 curPos;
  vec3 startPosWorldSpace = startPosWorldSpace_;
  vec3 endPosWorldSpace = endPosWorldSpace_;

  uint bSearchStepCount = 6u;
  for (uint i = 0u; i < bSearchStepCount; i++) {
    curPos = mix(startPosWorldSpace, endPosWorldSpace, 0.5);
    vec4 viewPos = cameraMatrixWorldInverse * vec4(curPos, 1.);
    float rayDepth = -viewPos.z;

    float rawSceneDepth = texture2D(sceneDepth, worldSpaceToScreenSpace(curPos)).x;
    float linearSceneDepth = linearizeDepth(rawSceneDepth, cameraNear, cameraFar);

    float depthDiff = rayDepth - linearSceneDepth;
    if (depthDiff > 0.) {
      endPosWorldSpace = curPos;
    } else {
      startPosWorldSpace = curPos;
    }
  }

  return mix(startPosWorldSpace, endPosWorldSpace, 0.5);
}

vec4 raymarchReflection(vec3 startPosWorldSpace, vec3 direction) {
  float baseStepSizeWorldSpace = 0.08;
  float targetStepSizeScreenSpace = 1. / 250.;
  float maxRayLength = cameraFar - cameraNear;

  float stepSizeWorldSpace = baseStepSizeWorldSpace;
  uint maxSteps = 250u;
  float minStepSizeWorldSpace = 0.000001;
  float maxStepSizeWorldSpace = maxRayLength / 250.;

  // TODO: It might be interesting to have a mipmapped depth texture to read from.
  //       That way, big regions can be skipped if they contain nothing remotely close.

  vec3 lastPosWorldSpace = startPosWorldSpace;
  vec3 curPosWorldSpace;
  bool hit = false;
  vec2 lastScreenPos = worldSpaceToScreenSpace(startPosWorldSpace);
  float lastDistanceBehindSurface = 100000.;
  for (uint i = 0u; i < maxSteps; i++) {
    curPosWorldSpace = lastPosWorldSpace + direction * stepSizeWorldSpace;
    float traveledDistance = distance(startPosWorldSpace, curPosWorldSpace);
    if (traveledDistance > maxRayLength) {
      break;
    }
    vec2 screenPos = worldSpaceToScreenSpace(curPosWorldSpace);

    // we want to try to take steps of a consistent size.
    float actualStepSizeScreenSpace = distance(screenPos, lastScreenPos);
    if (actualStepSizeScreenSpace < 0.0001) {
      return vec4(-1.);
    }
    float stepSizeScaleFactor = targetStepSizeScreenSpace / actualStepSizeScreenSpace;
    stepSizeWorldSpace = mix(stepSizeWorldSpace, stepSizeWorldSpace * stepSizeScaleFactor, 0.5);
    stepSizeWorldSpace = clamp(stepSizeWorldSpace, minStepSizeWorldSpace, maxStepSizeWorldSpace);

    if (screenPos.x < 0. || screenPos.x > 1. || screenPos.y < 0. || screenPos.y > 1.) {
      break;
    }

    float rawSceneDepth = texture2D(sceneDepth, screenPos).x;
    if (rawSceneDepth == 1.) {
      // TODO: Do we have to binary search for an edge here?
      lastPosWorldSpace = curPosWorldSpace;
      lastScreenPos = screenPos;
      lastDistanceBehindSurface = 100000.;
      continue;
    }
    float linearSceneDepth = linearizeDepth(rawSceneDepth, cameraNear, cameraFar);

    vec4 viewPos = cameraMatrixWorldInverse * vec4(curPosWorldSpace, 1.);
    float rayDepth = -viewPos.z;

    float distanceBehindSurface = rayDepth - linearSceneDepth;
    float rayDistanceThisStepWorldSpace = distance(lastPosWorldSpace, curPosWorldSpace);

    bool wasBehindSurface = lastDistanceBehindSurface > 0.;
    bool isBehindSurface = distanceBehindSurface > 0.;
    bool didIntersectSurface = isBehindSurface && !wasBehindSurface && distanceBehindSurface < rayDistanceThisStepWorldSpace * 2.;
    if (didIntersectSurface) {
      hit = true;
      // TODO: no idea if this is working.
      vec3 refinedPosWorldSpace = binarySearchHit(lastPosWorldSpace, curPosWorldSpace);
      // return vec4(abs(refinedPosWorldSpace - curPosWorldSpace), 1.);
      curPosWorldSpace = refinedPosWorldSpace;
      break;
    }

    lastPosWorldSpace = curPosWorldSpace;
    lastScreenPos = screenPos;
    lastDistanceBehindSurface = distanceBehindSurface;
  }

  if (!hit) {
    return vec4(-1.0);
  }

  vec2 reflectedColorScreenCoord = worldSpaceToScreenSpace(curPosWorldSpace);
  vec3 reflectedColor = texture2D(sceneDiffuse, reflectedColorScreenCoord).rgb;

  // fade out reflections from the edges of the screen to help hide discontinuities
  vec2 minDistanceToEdges = min(reflectedColorScreenCoord, 1. - reflectedColorScreenCoord);
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
  if (reflectedColor.x < -0.5) {
    gl_FragColor = diffuse;
    return;
    // reflectedColor = vec4(0., 0., 0., reflectionAlpha);
  }

  gl_FragColor = vec4(mix(diffuse.rgb, reflectedColor.rgb, reflectionAlpha * reflectedColor.a), 1.);

  // #include <dithering_fragment>
}
