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

const float fogMinY = -20.0;
const float fogMaxY = 55.0;
const int baseRaymarchStepCount = 200;
const int maxRaymarchStepCount = 300;
const float maxRayLength = 300.0;
const float minStepLength = 0.03;
const vec3 fogColor = vec3(0.68);
const int blueNoiseResolution = 470;

vec3 WorldPosFromDepth(float depth, vec2 coord) {
  float z = depth * 2.0 - 1.0;
  vec4 clipSpacePosition = vec4(coord * 2.0 - 1.0, z, 1.0);
  vec4 viewSpacePosition = cameraProjectionMatrixInv * clipSpacePosition;
  // Perspective division
  viewSpacePosition /= viewSpacePosition.w;
  vec4 worldSpacePosition = cameraMatrixWorld * viewSpacePosition;
  return worldSpacePosition.xyz;
}

// from: https://github.com/stegu/psrdnoise/blob/main/src/mpsrdnoise2.glsl
float mpsrdnoise(mediump vec2 x, mediump vec2 period, mediump float alpha, out mediump vec2 gradient) {
  mediump vec2 uv = vec2(x.x + x.y * 0.5, x.y);
  mediump vec2 i0 = floor(uv), f0 = fract(uv);
  mediump float cmp = step(f0.y, f0.x);
  mediump vec2 o1 = vec2(cmp, 1.0 - cmp);
  mediump vec2 i1 = i0 + o1, i2 = i0 + 1.0;
  mediump vec2 v0 = vec2(i0.x - i0.y * 0.5, i0.y);
  mediump vec2 v1 = vec2(v0.x + o1.x - o1.y * 0.5, v0.y + o1.y);
  mediump vec2 v2 = vec2(v0.x + 0.5, v0.y + 1.0);
  mediump vec2 x0 = x - v0, x1 = x - v1, x2 = x - v2;
  mediump vec3 iu, iv, xw, yw;
  if (any(greaterThan(period, vec2(0.0)))) {
    xw = vec3(v0.x, v1.x, v2.x);
    yw = vec3(v0.y, v1.y, v2.y);
    if (period.x > 0.0)
      xw = mod(vec3(v0.x, v1.x, v2.x), period.x);
    if (period.y > 0.0)
      yw = mod(vec3(v0.y, v1.y, v2.y), period.y);
    iu = floor(xw + 0.5 * yw + 0.5);
    iv = floor(yw + 0.5);
  } else {
    iu = vec3(i0.x, i1.x, i2.x);
    iv = vec3(i0.y, i1.y, i2.y);
  }
	// Hash permutation carefully tuned to stay within the range
	// of exact representation of integers in a half-float.
	// Tons of mod() operations here, sadly.
  mediump vec3 iu_m49 = mod(iu, 49.0);
  mediump vec3 iv_m49 = mod(iv, 49.0);
  mediump vec3 hashtemp = mod(14.0 * iu_m49 + 2.0, 49.0);
  hashtemp = mod(hashtemp * iu_m49 + iv_m49, 49.0);
  mediump vec3 hash = mod(14.0 * hashtemp + 4.0, 49.0);
  hash = mod(hash * hashtemp, 49.0);

  mediump vec3 psi = hash * 0.1282283 + alpha; // 0.1282283 is 2*pi/49
  mediump vec3 gx = cos(psi);
  mediump vec3 gy = sin(psi);
  mediump vec2 g0 = vec2(gx.x, gy.x);
  mediump vec2 g1 = vec2(gx.y, gy.y);
  mediump vec2 g2 = vec2(gx.z, gy.z);
  mediump vec3 w = 0.8 - vec3(dot(x0, x0), dot(x1, x1), dot(x2, x2));
  w = max(w, 0.0);
  mediump vec3 w2 = w * w;
  mediump vec3 w4 = w2 * w2;
  mediump vec3 gdotx = vec3(dot(g0, x0), dot(g1, x1), dot(g2, x2));
  mediump float n = dot(w4, gdotx);
  mediump vec3 w3 = w2 * w;
  mediump vec3 dw = -8.0 * w3 * gdotx;
  mediump vec2 dn0 = w4.x * g0 + dw.x * x0;
  mediump vec2 dn1 = w4.y * g1 + dw.y * x1;
  mediump vec2 dn2 = w4.z * g2 + dw.z * x2;
  gradient = 10.9 * (dn0 + dn1 + dn2);
  return 10.9 * n;
}

float sampleFogDensity(vec3 worldPos) {
  if (worldPos.y > fogMaxY) {
    return 0.;
  }

  // placeholder/debug: sine wave along the x and z axes, fading in from y=`fogStartY` to 0
  // float startY = fogStartY - sin(worldPos.x * 0.5) * sin(worldPos.z * 0.5) * 4.;
  // float transitionRange = 10.0;
  // float fogDensity = 1.0 - smoothstep(startY - transitionRange, startY, worldPos.y);
  // return fogDensity * 0.1;

  mediump vec2 gradient;
  float noise = mpsrdnoise(worldPos.xz * 0.2, vec2(0.0), 0.0, gradient);

  // scale to [0, 1]
  noise = noise * 0.5 + 0.5;
  return pow(noise, 4.) * 0.08 * sin(worldPos.y * 0.1);
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

vec4 march(in vec3 startPos, in vec3 endPos, in ivec2 screenCoord) {
  // If the ray we're marching never intersects the fog, we can skip the whole thing.
  if (startPos.y > fogMaxY && endPos.y > fogMaxY) {
    return vec4(0.0);
  }

  vec3 rayDir = normalize(endPos - startPos);
  float rayLength = min(length(endPos - startPos), maxRayLength);

  // TODO: investigate dynamic step length
  float baseStepLength = rayLength / float(baseRaymarchStepCount);

  // clip the march segment endpoints to minimize the length of the ray
  clipRayEndpoints(startPos, endPos);

  // use blue noise to jitter the ray
  float blueNoiseSample = texelFetch(blueNoise, screenCoord % blueNoiseResolution, 0).r;
  float jitter = (blueNoiseSample - 0.5) * 0.1;
  startPos += rayDir * jitter;

  float density = 0.0;
  float totalDistance = 0.;
  int totalIters = 0;
  while (totalDistance < rayLength) {
    totalIters += 1;
    if (totalIters > maxRaymarchStepCount) {
      break;
    }

    float stepLength = max(baseStepLength, minStepLength);
    totalDistance += stepLength;
    vec3 pos = startPos + rayDir * totalDistance;
    float fogDensity = sampleFogDensity(pos);
    density += fogDensity * stepLength;

    // TODO: lighting

    if (density > 1.0) {
      break;
    }
  }

  // debug step count
  // return vec4(vec3(float(totalIters) / float(maxRaymarchStepCount)), 1.0);

  return vec4(fogColor, clamp(density, 0., 1.));
}

void main() {
  float depth = texture2D(sceneDepth, vUv).x;

  vec3 worldPos = WorldPosFromDepth(depth, vUv);
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
