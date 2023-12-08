/*
 * References:
 *
 * - https://www.shadertoy.com/view/XslGRr
 * - https://github.com/Ameobea/three-good-godrays/blob/main/src/godrays.frag
 * - https://n8python.github.io/goodGodRays/
 */

in vec2 vUv;

uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;
uniform sampler2D blueNoise;
uniform lowp sampler3D noiseTexture;
uniform ivec2 resolution;
uniform vec3 cameraPos;
uniform mat4 cameraProjectionMatrixInv;
uniform mat4 cameraMatrixWorld;
uniform float curTimeSeconds;

#define DO_LIGHTING 0
#define USE_LIGHT_FALLOFF 1
#define OCTAVE_COUNT 6

// params
uniform float ambientLightIntensity;
uniform vec3 ambientLightColor;
uniform float fogMinY;
uniform float fogMaxY;
uniform int baseRaymarchStepCount;
uniform int maxRaymarchStepCount;
uniform float maxRayLength;
uniform float minStepLength;
uniform float maxDensity;
uniform vec3 fogColorHighDensity;
uniform vec3 fogColorLowDensity;
uniform vec3 lightColor;
uniform float lightIntensity;
uniform int blueNoiseResolution;
uniform float lightFalloffDistance;
uniform float fogFadeOutPow;
uniform float fogFadeOutRangeY;
uniform float fogDensityMultiplier;
uniform float heightFogStartY;
uniform float heightFogEndY;
uniform float heightFogFactor;
uniform float noiseBias;
uniform float noisePow;
uniform vec2 noiseMovementPerSecond;
uniform float postDensityMultiplier;
uniform float postDensityPow;
uniform float globalScale;

vec3 computeWorldPosFromDepth(float depth, vec2 coord) {
  float z = depth * 2.0 - 1.0;
  vec4 clipSpacePosition = vec4(coord * 2. - 1., z, 1.);
  vec4 viewSpacePosition = cameraProjectionMatrixInv * clipSpacePosition;
  // Perspective division
  viewSpacePosition /= viewSpacePosition.w;
  vec4 worldSpacePosition = cameraMatrixWorld * viewSpacePosition;
  return worldSpacePosition.xyz;
}

float sampleFogFromNoise(vec3 worldPos) {
  return texture(noiseTexture, worldPos * 0.006).r * 2. - 1.;
}

#define TOTAL_LOD_COUNT 6
const float LODWeights[TOTAL_LOD_COUNT] = float[](1., 0.3, 0.2, 0.1, 0.06, 0.035);
const float LODScales[TOTAL_LOD_COUNT] = float[](0.1, 0.3, 0.6, 1.2, 2.2, 4.1);

float sampleFogDensityLOD(vec3 worldPos, float distanceToCamera, inout float totalSampledMagnitude, const int lod) {
  float weight = LODWeights[lod];
  float scale = LODScales[lod] * globalScale;

  totalSampledMagnitude += weight;
  float noise = sampleFogFromNoise(worldPos * scale);
  return noise * weight;
}

float computeFadeOutYFactor(float y) {
  return 1. - pow(smoothstep(fogMaxY - fogFadeOutRangeY, fogMaxY, y), fogFadeOutPow);
}

float sampleFogDensity(vec3 worldPos, out vec3 gradient, float distanceToCamera) {
  // TODO: compute gradient
  float noise = noiseBias;

  worldPos += vec3(noiseMovementPerSecond.x, 0., noiseMovementPerSecond.y) * curTimeSeconds;

  // keep track of total magnitude of sampled weights so noise can be properly normalized
  float totalSampledMagnitude = 0.;
  for (int octave = 0; octave < OCTAVE_COUNT; octave++) {
    noise += sampleFogDensityLOD(worldPos, distanceToCamera, totalSampledMagnitude, octave);
  }

  // \/ Uncommenting this seems to wash out detail in the fog, so leaving it off even though it's probably more correct
  // noise /= totalSampledMagnitude;

  noise = noise * 0.5 + 0.5;
  noise = clamp(noise, -1., 1.);

  noise = pow(noise, noisePow);

  // linear height fog, getting denser as the y coordinate decreases
  if (worldPos.y <= heightFogEndY) {
    float noiseFactor = 1. - smoothstep(heightFogStartY, heightFogEndY, worldPos.y);
    noise += noiseFactor * heightFogFactor;
  }

  // fade fog out up to the max fog height
  float fadeOutFactor = computeFadeOutYFactor(worldPos.y);
  noise *= fadeOutFactor;

  // scale the noise to match the desired density range
  noise = noise * fogDensityMultiplier;

  return noise;
}

/**
 * Calculates the signed distance from point `p` to a plane defined by
 * normal `n` and distance `h` from the origin.
 *
 * `n` must be normalized.
 */
float sdPlane(vec3 p, vec3 n, float h) {
  return dot(p, n) + h;
}

/**
 * Calculates the intersection of a ray defined by `rayOrigin` and `rayDirection`
 * with a plane defined by normal `planeNormal` and distance `planeDistance`
 *
 * Returns the distance from the ray origin to the intersection point.
 *
 * The return value will be negative if the ray does not intersect the plane.
 */
float intersectRayPlane(vec3 rayOrigin, vec3 rayDirection, vec3 planeNormal, float planeDistance) {
  float denom = dot(planeNormal, rayDirection);
  return -(sdPlane(rayOrigin, planeNormal, planeDistance) / denom);
}

/**
 * Specialized version of `intersectRayPlane` that assumes the plane is the XZ plane, i.e. normal = (0, 1, 0)
 */
float intersectRayXZPlane(vec3 rayOrigin, vec3 rayDirection, float planeDistance) {
  return -(rayOrigin.y - planeDistance) / rayDirection.y;
}

void clipRayEndpoints(inout vec3 startPos, inout vec3 endPos) {
  // Check if the entire segment is out of the fog range
  if ((startPos.y < fogMinY && endPos.y < fogMinY) || (startPos.y > fogMaxY && endPos.y > fogMaxY)) {
    startPos = endPos;
    return;
  }

  vec3 rayDir = normalize(endPos - startPos);

  // Find intersection with fogMinY plane
  if (startPos.y < fogMinY) {
    float t = intersectRayXZPlane(startPos, rayDir, fogMinY);
    startPos = startPos + rayDir * t;
  }
  if (endPos.y < fogMinY) {
    float t = intersectRayXZPlane(startPos, rayDir, fogMinY);
    endPos = startPos + rayDir * t;
  }

  // Find intersection with fogMaxY plane
  if (endPos.y > fogMaxY) {
    float t = intersectRayXZPlane(startPos, rayDir, fogMaxY);
    endPos = startPos + rayDir * t;
  }
  if (startPos.y > fogMaxY) {
    float t = intersectRayXZPlane(startPos, rayDir, fogMaxY);
    startPos = startPos + rayDir * t;
  }
}

vec3 computeColor(in vec3 curPos, in float density, in vec3 gradient) {
  vec3 baseFogColor = mix(fogColorLowDensity, fogColorHighDensity, clamp(density * 12., 0., 1.));
  #if !DO_LIGHTING
  return baseFogColor;
  #endif

  // TODO
  vec3 normal = vec3(0., 1., 0.);
  // vec3 normal = normalize(gradient);

  vec3 realLightPos = vec3(sin(curTimeSeconds * 0.5) * 50., 7., 0.);
  float diffuseFactor = clamp(dot(normal, normalize(realLightPos - curPos)), 0., 1.);
  #if USE_LIGHT_FALLOFF
  diffuseFactor *= 1. - smoothstep(0., lightFalloffDistance, length(realLightPos - curPos));
  #endif

  vec3 ambientColor = ambientLightColor * ambientLightIntensity;
  vec3 diffuseColor = lightColor * lightIntensity * diffuseFactor;
  return baseFogColor * (ambientColor + diffuseColor);
}

void sampleOneStep(
  in float totalDistance,
  in float stepSize,
  in vec3 startPos,
  in vec3 rayDir,
  inout vec3 gradient,
  inout vec3 accumulatedColor,
  inout float accumulatedDensity
) {
  vec3 curPos = startPos + rayDir * totalDistance;
  float distanceToCamera = length(curPos - cameraPos);
  float rawDensity = sampleFogDensity(curPos, gradient, distanceToCamera);
  float density = rawDensity * stepSize;

  if (rawDensity > 0.005) {
    vec3 color = computeColor(curPos, rawDensity, gradient);
    // We only accumulate within the remaining "space" of the transparency so far
    float remainingOpacity = 1. - accumulatedDensity;
    accumulatedColor += color * density * remainingOpacity;
    accumulatedDensity += density * remainingOpacity;
  }
}

vec4 march(in vec3 startPos, in vec3 endPos, in ivec2 screenCoord) {
  float beforeLength = length(endPos - startPos);

  // clip the march segment endpoints to minimize the length of the ray
  clipRayEndpoints(startPos, endPos);

  // debug clipped ray length ratio
  // return vec4(vec3(length(endPos - startPos) / beforeLength), 1.0);

  // This indicates that the entire ray is outside of the fog zone, so we can
  // skip marching alltogether.
  if (startPos == endPos) {
    return vec4(0.0);
  }

  vec3 rayDir = normalize(endPos - startPos);
  float rayLength = length(endPos - startPos);

  // TODO: When rays are at very slight angles to the bounding planes and a relatively high `fogFadeOutRangeY` is set,
  // the ray will be clipped to spend most of its length within the attenuation zone.  This causes the final density
  // after marching to be low since the region where the ray has most if its density is not marched.
  //
  // To fix this, the clip zone for long rays that interact with that attenuation zone should be adjusted to start the
  // ray deeper within the fog volume so that a more accurate density can be computed.
  if (rayLength > maxRayLength) {
    rayLength = maxRayLength;
  }

  // debug ray length
  // return vec4(vec3(rayLength / maxRayLength), 1.0);

  // debug ray start y
  // return vec4(vec3(smoothstep(fogMinY, fogMaxY, startPos.y)), 1.0);

  // debug ray end y
  // return vec4(vec3(smoothstep(fogMinY, fogMaxY, endPos.y)), 1.0);

  float baseStepLength = rayLength / float(baseRaymarchStepCount);

  // use blue noise to jitter the ray
  float blueNoiseSample = texelFetch(blueNoise, screenCoord % blueNoiseResolution, 0).r;
  float jitter = (blueNoiseSample - 0.5) * 0.1;
  startPos += rayDir * jitter;

  float density = 0.;
  float totalDistance = 0.;
  int totalIters = 0;
  vec3 gradient;
  vec3 accumulatedColor = vec3(0.);
  while (totalDistance < rayLength) {
    // safety check to avoid infinite march bugs and similar
    if (totalIters > maxRaymarchStepCount) {
      return vec4(1., 1., 0., 1.);
      // break;
    }
    totalIters += 1;

    float stepLength = max(baseStepLength, minStepLength);
    // I've found that adding some jitter to the step length helps to reduce artifacts in very
    // short rays, and allows the min step length to be set higher, which improves performance.
    // stepLength += jitter * 0.2;
    totalDistance += stepLength;
    sampleOneStep(totalDistance, stepLength, startPos, rayDir, gradient, accumulatedColor, density);

    if (density >= (maxDensity - 0.01)) {
      // density = min(density, maxDensity);
      break;
    }
  }

  // debug density
  // return vec4(vec3(density), 1.0);
  // if (density > 0.5) {
  //   return vec4((density - 0.5) * 2., 0., 0., 1.);
  // } else {
  //   return vec4(0., 0., density * 2., 1.);
  // }

  // debug fog color
  // return vec4(accumulatedColor, 1.);

  // debug step count
  // return vec4(vec3(float(totalIters) / float(baseRaymarchStepCount)), 1.0);

  density = pow(density * postDensityMultiplier, postDensityPow);

  return clamp(vec4(accumulatedColor, density), 0., 1.);
}

void main() {
  float depth = texture2D(sceneDepth, vUv).x;

  vec3 worldPos = computeWorldPosFromDepth(depth, vUv);
  vec3 rayStartPos = cameraPos;
  vec3 rayEndPos = worldPos;

  ivec2 screenCoord = ivec2(vUv * vec2(resolution));
  vec4 fogColor = march(rayStartPos, rayEndPos, screenCoord);

  #ifdef DO_DIRECT_COMPOSITING
  vec3 diffuse = texture2D(sceneDiffuse, vUv).rgb;
  // composite the fog color over the diffuse color using the density stored in the alpha channel
  gl_FragColor = vec4(mix(diffuse, fogColor.rgb, fogColor.a), 1.0);
  #else
  gl_FragColor = fogColor.rgba;
  #endif
}
