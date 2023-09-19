/*
 * References:
 *
 * - https://github.com/Ameobea/three-good-godrays/blob/main/src/godrays.frag
 * - https://n8python.github.io/goodGodRays/
 */

in vec2 vUv;

uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;
uniform sampler2D blueNoise;
uniform ivec2 resolution;
uniform vec3 cameraPos;
uniform mat4 cameraProjectionMatrixInv;
uniform mat4 cameraMatrixWorld;
uniform float curTimeSeconds;

#define USE_GRADIENT_BASED_DYNAMIC_STEP_SIZE 0
#define DO_LIGHTING 1
#define USE_LIGHT_FALLOFF 1
#define USE_LOD 0
#define OCTAVE_COUNT 3

const float fogMinY = -4.0;
const float fogMaxY = 4.4;
const int baseRaymarchStepCount = 120;
const int maxRaymarchStepCount = 200;
const float maxRayLength = 300.0;
const float minStepLength = 0.4;
const float maxDensity = 1.;
const vec3 fogColorHighDensity = vec3(0.5);
const vec3 fogColorLowDensity = vec3(0.8);
const vec3 lightColor = vec3(1.0, 0.0, 0.97);
const float lightIntensity = 0.5;
const int blueNoiseResolution = 470;
const vec3 ambientLightColor = vec3(0.62);
const float ambientLightIntensity = 0.9;
const float lightFalloffDistance = 110.;
const float fogFadeOutPow = 2.;
const float fogFadeOutRangeY = 1.5;
const float fogDensityMultiplier = 0.086;
const float heightFogFactor = 0.0852;
const float noiseBias = 0.885;

vec3 computeWorldPosFromDepth(float depth, vec2 coord) {
  float z = depth * 2.0 - 1.0;
  vec4 clipSpacePosition = vec4(coord * 2. - 1., z, 1.);
  vec4 viewSpacePosition = cameraProjectionMatrixInv * clipSpacePosition;
  // Perspective division
  viewSpacePosition /= viewSpacePosition.w;
  vec4 worldSpacePosition = cameraMatrixWorld * viewSpacePosition;
  return worldSpacePosition.xyz;
}

float psrdnoise(vec2 x, vec2 period, float alpha, out vec2 gradient) {
  // Transform to simplex space (axis-aligned hexagonal grid)
  vec2 uv = vec2(x.x + x.y * 0.5, x.y);

  // Determine which simplex we're in, with i0 being the "base"
  vec2 i0 = floor(uv);
  vec2 f0 = fract(uv);
  // o1 is the offset in simplex space to the second corner
  float cmp = step(f0.y, f0.x);
  vec2 o1 = vec2(cmp, 1.0 - cmp);

  // Enumerate the remaining simplex corners
  vec2 i1 = i0 + o1;
  vec2 i2 = i0 + vec2(1.0, 1.0);

  // Transform corners back to texture space
  vec2 v0 = vec2(i0.x - i0.y * 0.5, i0.y);
  vec2 v1 = vec2(v0.x + o1.x - o1.y * 0.5, v0.y + o1.y);
  vec2 v2 = vec2(v0.x + 0.5, v0.y + 1.0);

  // Compute vectors from v to each of the simplex corners
  vec2 x0 = x - v0;
  vec2 x1 = x - v1;
  vec2 x2 = x - v2;

  vec3 iu = vec3(0.0);
  vec3 iv = vec3(0.0);
  vec3 xw = vec3(0.0);
  vec3 yw = vec3(0.0);

  // Wrap to periods, if desired
  if (any(greaterThan(period, vec2(0.0)))) {
    xw = vec3(v0.x, v1.x, v2.x);
    yw = vec3(v0.y, v1.y, v2.y);
    if (period.x > 0.0)
      xw = mod(vec3(v0.x, v1.x, v2.x), period.x);
    if (period.y > 0.0)
      yw = mod(vec3(v0.y, v1.y, v2.y), period.y);
    // Transform back to simplex space and fix rounding errors
    iu = floor(xw + 0.5 * yw + 0.5);
    iv = floor(yw + 0.5);
  } else { // Shortcut if neither x nor y periods are specified
    iu = vec3(i0.x, i1.x, i2.x);
    iv = vec3(i0.y, i1.y, i2.y);
  }

  // Compute one pseudo-random hash value for each corner
  vec3 hash = mod(iu, 289.0);
  hash = mod((hash * 51.0 + 2.0) * hash + iv, 289.0);
  hash = mod((hash * 34.0 + 10.0) * hash, 289.0);

  // Pick a pseudo-random angle and add the desired rotation
  vec3 psi = hash * 0.07482 + alpha;
  vec3 gx = cos(psi);
  vec3 gy = sin(psi);

  // Reorganize for dot products below
  vec2 g0 = vec2(gx.x, gy.x);
  vec2 g1 = vec2(gx.y, gy.y);
  vec2 g2 = vec2(gx.z, gy.z);

  // Radial decay with distance from each simplex corner
  vec3 w = 0.8 - vec3(dot(x0, x0), dot(x1, x1), dot(x2, x2));
  w = max(w, 0.0);
  vec3 w2 = w * w;
  vec3 w4 = w2 * w2;

  // The value of the linear ramp from each of the corners
  vec3 gdotx = vec3(dot(g0, x0), dot(g1, x1), dot(g2, x2));

  // Multiply by the radial decay and sum up the noise value
  float n = dot(w4, gdotx);

  // Compute the first order partial derivatives
  vec3 w3 = w2 * w;
  vec3 dw = -8.0 * w3 * gdotx;
  vec2 dn0 = w4.x * g0 + dw.x * x0;
  vec2 dn1 = w4.y * g1 + dw.y * x1;
  vec2 dn2 = w4.z * g2 + dw.z * x2;
  gradient = 10.9 * (dn0 + dn1 + dn2);

  // Scale the return value to fit nicely into the range [-1,1]
  return 10.9 * n;
}

#define TOTAL_LOD_COUNT 4
const float LODWeights[TOTAL_LOD_COUNT] = float[](1., 0.4, 0.2, 0.1);
const float LODScales[TOTAL_LOD_COUNT] = float[](0.1, 0.3, 0.6, 1.2);
const vec2 LODShutoffZoneRanges[TOTAL_LOD_COUNT] = vec2[](vec2(999999999., 999999999.), vec2(50., 80.), vec2(10., 40.), vec2(10., 40.));

float sampleFogDensityLOD(vec3 worldPos, out vec3 gradient, float distanceToCamera, inout float totalSampledMagnitude, const int lod) {
  #if USE_LOD
  vec2 shutoffZone = LODShutoffZoneRanges[lod];
  float shutoff = smoothstep(shutoffZone.x, shutoffZone.y, distanceToCamera);
  float activation = 1. - shutoff;
  if (activation == 0.) {
    return 0.;
  }
  #else
  float activation = 1.;
  #endif

  float weight = LODWeights[lod];
  float scale = LODScales[lod];

  vec2 xzGradient;
  float noise = psrdnoise(worldPos.xz * scale, vec2(0.), 0., xzGradient);
  gradient += vec3(xzGradient.x, 0., xzGradient.y) * weight * activation;
  totalSampledMagnitude += weight * activation;
  return noise * weight * activation;
}

float computeFadeOutYFactor(float y) {
  return pow(1. - smoothstep(fogMaxY - fogFadeOutRangeY, fogMaxY, y), fogFadeOutPow);
}

float computeFadeOutYDerivative(float y) {
  if (y <= fogMaxY - fogFadeOutRangeY || y >= fogMaxY) {
    return 0.;
  }

  float t = (y - (fogMaxY - fogFadeOutRangeY)) / fogFadeOutRangeY;

  // Derivative of smoothstep function
  float dSmoothStep = 6. * t * (1. - t) / fogFadeOutRangeY;

  // Chain rule to compute the derivative considering the power term
  float fadeOutTerm = 1. - smoothstep(fogMaxY - fogFadeOutRangeY, fogMaxY, y);
  float fadeOutDerivative = -fogFadeOutPow * pow(fadeOutTerm, fogFadeOutPow - 1.) * dSmoothStep;

  return fadeOutDerivative;
}

float sampleFogDensity(vec3 worldPos, out vec3 gradient, float distanceToCamera) {
  gradient = vec3(0.);
  float noise = noiseBias;

  worldPos += vec3(curTimeSeconds * 0.8, 0., 0.);

  // keep track of total magnitude of sampled weights so noise can be properly normalized
  float totalSampledMagnitude = 0.;
  for (int octave = 0; octave < OCTAVE_COUNT; octave++) {
    noise += sampleFogDensityLOD(worldPos, gradient, distanceToCamera, totalSampledMagnitude, octave);
  }

  // fade fog out up to the max fog height
  float fadeOut = computeFadeOutYFactor(worldPos.y);
  float fadeOutYDerivative = computeFadeOutYDerivative(worldPos.y);
  gradient.y = noise * fadeOutYDerivative;
  // Scale the x and z components of the gradient by the fade-out factor
  gradient.x *= fadeOut;
  gradient.z *= fadeOut;
  noise *= fadeOut;

  // scale the noise from [-totalSampledMagnitude, totalSampledMagnitude] to [-1, 1]
  float normalizationScale = 1. / totalSampledMagnitude;
  noise = noise * normalizationScale;
  // scale the noise from [-1, 1] to [0, 1]
  noise = noise * 0.5 + 0.5;
  // scale the gradient as well to match the noise
  gradient *= normalizationScale * 0.5;

  noise = pow(noise, 3.);
  gradient *= 3. * pow(noise, 2.);

  // scale the noise to match the desired density range
  noise = noise * fogDensityMultiplier;
  gradient *= fogDensityMultiplier;

  // linear height fog, getting denser as the y coordinate decreases
  if (worldPos.y <= 3.) {
    const float heightFogStartY = 0.;
    const float heightFogEndY = 3.;
    const float heightFogRange = heightFogEndY - heightFogStartY;
    float noiseFactor = 1. - smoothstep(heightFogStartY, heightFogEndY, worldPos.y);
    noise += noiseFactor * heightFogFactor;
    if (worldPos.y >= 0.) {
      gradient.y += heightFogFactor; //  / heightFogRange;
    }
  }

  return min(noise, 1.);
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

float computeStepLengthMultiplier(in vec3 gradient) {
  return 3. - 2. * smoothstep(0., 0.03, length(gradient));
}

vec3 computeColor(in vec3 curPos, in float density, in vec3 gradient) {
  vec3 baseFogColor = mix(fogColorLowDensity, fogColorHighDensity, clamp(density * 10., 0., 1.));
  #if !DO_LIGHTING
  return baseFogColor;
  #endif

  vec3 normal = normalize(gradient);
  if (curPos.y > fogMaxY - fogFadeOutRangeY) {
    normal = vec3(0., 1., 0.);
  }

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
  inout float totalDistance,
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

  if (density > 0.01) {
    vec3 color = computeColor(curPos, density, gradient);
    color *= density;
    // We only accumulate within the remaining "space" of the transparency so far
    float remainingOpacity = 1. - accumulatedDensity;
    accumulatedColor += color * remainingOpacity;
    accumulatedDensity += density * remainingOpacity;
  }
}

vec4 march(in vec3 startPos, in vec3 endPos, in ivec2 screenCoord) {
  // clip the march segment endpoints to minimize the length of the ray
  clipRayEndpoints(startPos, endPos);

  // This indicates that the entire ray is outside of the fog zone, so we can
  // skip marching alltogether.
  if (startPos == endPos) {
    return vec4(0.0);
  }

  vec3 rayDir = normalize(endPos - startPos);
  float rayLength = length(endPos - startPos);

  // Assume that if the ray is that long, it will saturate to the fog color
  if (rayLength > maxRayLength) {
    // return vec4(computeColor(endPos, maxDensity, vec3(0., 1., 0.)) * maxDensity, maxDensity);
    rayLength = maxRayLength;
  }

  // debug ray length
  // return vec4(vec3(rayLength / maxRayLength), 1.0);

  // debug ray start y
  // return vec4(vec3(smoothstep(fogMinY, fogMaxY, startPos.y)), 1.0);

  // debug ray end y
  // return vec4(vec3(smoothstep(fogMinY, fogMaxY, endPos.y)), 1.0);

  // TODO: investigate dynamic step length
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
  float stepLengthMultiplier = 1.;
  while (totalDistance < rayLength) {
    // safety check to avoid infinite march bugs and similar
    if (totalIters > maxRaymarchStepCount) {
      return vec4(1., 0., 0., 1.);
      // break;
    }
    totalIters += 1;

    float stepLength = max(baseStepLength * stepLengthMultiplier, minStepLength);
    // I've found that adding some jitter to the step length helps to reduce artifacts in very
    // short rays, and allows the min step length to be set higher, which improves performance.
    stepLength += jitter * 0.2;
    totalDistance += stepLength;
    sampleOneStep(totalDistance, stepLength, startPos, rayDir, gradient, accumulatedColor, density);

    if (density >= (maxDensity - 0.01)) {
      break;
    }

    #if USE_GRADIENT_BASED_DYNAMIC_STEP_SIZE
    // increase step size if gradient is small
    stepLengthMultiplier = computeStepLengthMultiplier(gradient);
    #endif
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

  return clamp(vec4(accumulatedColor, density), 0., 1.);
}

void main() {
  float depth = texture2D(sceneDepth, vUv).x;

  vec3 worldPos = computeWorldPosFromDepth(depth, vUv);
  vec3 rayStartPos = cameraPos;
  vec3 rayEndPos = worldPos;

  ivec2 screenCoord = ivec2(vUv * vec2(resolution));
  vec4 fogColor = march(rayStartPos, rayEndPos, screenCoord);
  // TODO: If lower resolution or any blur support is needed, this will have to be done in a separate pass
  vec3 diffuse = texture2D(sceneDiffuse, vUv).rgb;
  // composite the fog color over the diffuse color using the density stored in the alpha channel
  vec3 color = mix(diffuse, fogColor.rgb, fogColor.a);

  gl_FragColor = vec4(color, 1.0);
}
