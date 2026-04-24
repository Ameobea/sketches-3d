// ---- Grid cell constants ----
const int   CELL_SIZE_X_$ID    = 22;    // grid cell period in voxels (X)
const int   CELL_SIZE_Z_$ID    = 20;    // grid cell period in voxels (Z)
const int   VOLUME_DEPTH_$ID   = 22;    // number of voxel layers (Y)
const int   TRENCH_WIDTH_X_$ID = 3;     // trench width in voxels (X)
const int   TRENCH_WIDTH_Z_$ID = 6;     // trench width in voxels (Z)
const int   TRENCH_DEPTH_$ID   = 20;    // layers carved from top for trenches
const float GROUND_HEIGHT_$ID  = 120.0; // virtual distance to volume top
const float VOXEL_SIZE_$ID     = 4.0;   // world-space size of one voxel

// ---- Appearance ----
const vec3  SURFACE_COLOR_$ID    = vec3(0.008, 0.008, 0.008);
const vec3  LAVA_COLOR_$ID       = vec3(1.0, 0.177, 0.0);
const float LAVA_INTENSITY_$ID   = 0.25;
const float LIGHT_INTENSITY_$ID  = 0.4;
const vec3  LIGHT_DIR_$ID        = vec3(0.4243, 0.8485, 0.3182); // normalize(0.4, 0.8, 0.3)
const float AMBIENT_$ID          = 0.0;

// ---- Parallax & distance fog ----
const float PARALLAX_SCALE_$ID     = 0.25;
const float FOG_START_$ID          = 200.0;  // world-space distance where fade begins
const float FOG_END_$ID            = 8200.0; // world-space distance where fully transparent
const float FOG_EMISSIVE_ATTEN_$ID = 0.4;    // extra emissive attenuation from fog (0–1)

// Integer mod that always returns a non-negative result.
// GLSL's `%` follows C99 sign-of-dividend semantics, so -1 % 5 == -1.
int posMod_$ID(int a, int b) {
  return ((a % b) + b) % b;
}

// Decompose world voxel coordinates into grid cell index and cell-local position.
void cellDecompose_$ID(int wx, int wz, out ivec2 cellIx, out ivec2 localXZ) {
  localXZ = ivec2(posMod_$ID(wx, CELL_SIZE_X_$ID), posMod_$ID(wz, CELL_SIZE_Z_$ID));
  cellIx = ivec2((wx - localXZ.x) / CELL_SIZE_X_$ID, (wz - localXZ.y) / CELL_SIZE_Z_$ID);
}

// Returns true if the voxel at localPos within grid cell cellIx is solid.
// localPos.xz is in [0, cellSize), localPos.y is the global Y layer.
bool isFilled_$ID(ivec2 cellIx, ivec3 localPos) {
  bool isTrenchX = localPos.x < TRENCH_WIDTH_X_$ID;
  bool isTrenchZ = localPos.z < TRENCH_WIDTH_Z_$ID;

  // 2 base hashes, 4 derived via cheap ALU (saves ~4 hash() calls per DDA step).
  vec2 cv = vec2(cellIx);
  float h1 = hash(cv + vec2(0.5, 0.5));
  float h2 = hash(cv + vec2(7.3, 13.1));

  int bridgeHeightOffset = int(floor(h1 * 8.0));
  if (isTrenchX && isTrenchZ && abs(localPos.y - (VOLUME_DEPTH_$ID - (18 - bridgeHeightOffset))) < 2) {
    return true;
  }

  if (isTrenchX || isTrenchZ) {
    return localPos.y < VOLUME_DEPTH_$ID - TRENCH_DEPTH_$ID;
  }

  int ceiling = VOLUME_DEPTH_$ID - 2 - int(floor(h2 * 10.0));

  // Derived hashes: fract(a*h1 + b*h2) with irrational-ish coefficients.
  float hGrooveX = fract(h1 * 5.37 + h2 * 8.71);
  float hGrooveZ = fract(h1 * 11.13 + h2 * 3.79);
  int grooveX = int(hGrooveX * float(CELL_SIZE_X_$ID + 4));
  int grooveZ = int(hGrooveZ * float(CELL_SIZE_Z_$ID + 4));
  // Width: binary decisions from different threshold splits of base hashes.
  int grooveXWidth = h1 > 0.4 ? 1 : 2;
  int grooveZWidth = h2 > 0.6 ? 1 : 2;

  bool isGroove = (abs(localPos.x - grooveX) < grooveXWidth) || (abs(localPos.z - grooveZ) < grooveZWidth);
  return localPos.y <= (ceiling - (isGroove ? 2 : 0));
}

struct VoxelHit_$ID {
  bool hit;
  ivec3 cell;
  vec3 normal;
  vec3 hitPos;   // voxel-space position on the hit face
};

VoxelHit_$ID traceVoxels_$ID(vec3 ro, vec3 rd) {
  VoxelHit_$ID result;
  result.hit = false;
  result.cell = ivec3(0);
  result.normal = vec3(0.0, 1.0, 0.0);
  result.hitPos = ro;

  ivec3 cell = ivec3(floor(ro));
  if (cell.y >= VOLUME_DEPTH_$ID) cell.y = VOLUME_DEPTH_$ID - 1;

  ivec3 stepDir = ivec3(
    rd.x >= 0.0 ? 1 : -1,
    rd.y >= 0.0 ? 1 : -1,
    rd.z >= 0.0 ? 1 : -1
  );

  vec3 tDelta = vec3(
    abs(rd.x) > 1e-8 ? abs(1.0 / rd.x) : 1e30,
    abs(rd.y) > 1e-8 ? abs(1.0 / rd.y) : 1e30,
    abs(rd.z) > 1e-8 ? abs(1.0 / rd.z) : 1e30
  );

  vec3 tMax = vec3(
    abs(rd.x) > 1e-8 ? (float(cell.x + (stepDir.x > 0 ? 1 : 0)) - ro.x) / rd.x : 1e30,
    abs(rd.y) > 1e-8 ? (float(cell.y + (stepDir.y > 0 ? 1 : 0)) - ro.y) / rd.y : 1e30,
    abs(rd.z) > 1e-8 ? (float(cell.z + (stepDir.z > 0 ? 1 : 0)) - ro.z) / rd.z : 1e30
  );

  vec3 lastNormal = vec3(0.0, 1.0, 0.0);
  float lastT = 0.0;

  for (int i = 0; i < MAX_VOX_DDA_STEPS; i++) {
    if (cell.y < 0 || cell.y >= VOLUME_DEPTH_$ID) break;

    ivec2 cellIx, localXZ;
    cellDecompose_$ID(cell.x, cell.z, cellIx, localXZ);

    if (isFilled_$ID(cellIx, ivec3(localXZ.x, cell.y, localXZ.y))) {
      result.hit = true;
      result.cell = cell;
      result.normal = lastNormal;
      result.hitPos = ro + lastT * rd;
      return result;
    }

    // Advance to the nearest axis-aligned cell boundary.
    if (tMax.x < tMax.y && tMax.x < tMax.z) {
      lastT = tMax.x;
      lastNormal = vec3(-float(stepDir.x), 0.0, 0.0);
      cell.x += stepDir.x;
      tMax.x += tDelta.x;
    } else if (tMax.y < tMax.z) {
      lastT = tMax.y;
      lastNormal = vec3(0.0, -float(stepDir.y), 0.0);
      cell.y += stepDir.y;
      tMax.y += tDelta.y;
    } else {
      lastT = tMax.z;
      lastNormal = vec3(0.0, 0.0, -float(stepDir.z));
      cell.z += stepDir.z;
      tMax.z += tDelta.z;
    }
  }

  return result;
}

float fbm_3_octaves(vec3 x) {
	float v = 0.;
	float a = 0.5;
	vec3 shift = vec3(100);
	for (int i = 0; i < 3; ++i) {
		v += a * noise(x);
		x = x * 2.5 + shift;
		a *= 0.45;
	}
	return v;
}

void colorLava_$ID(
  VoxelHit_$ID hit,
  out vec3 outColor,
  out vec3 outEmissive,
  out float outEmissiveAlpha
) {
  vec3 lavaCol;

#if VOX_LAVA_QUALITY >= 1
  // High quality: 3D fbm noise between two lava tones with directional flow.
  const vec3 LAVA_HOT_$ID  = vec3(1.0, 0.177, 0.0);
  const vec3 LAVA_COOL_$ID = vec3(0.7, 0.08, 0.0);

  // World-space XZ for continuity across voxel faces.
  vec2 worldUV = hit.hitPos.xz;
  // Directional flow in XZ + slow Z animation for organic evolution.
  vec2 flowDir = vec2(-0.35, 0.);
  vec3 noiseCoord = vec3((worldUV + flowDir * uTime) * 0.33, uTime * 0.12);
  float n = smoothstep(0.16, 0.9, fbm_3_octaves(noiseCoord) * 0.94);
  lavaCol = mix(LAVA_COOL_$ID, LAVA_HOT_$ID, n);
#else
  // Medium quality: single noise octave between two tones with directional flow.
  const vec3 LAVA_HOT_$ID  = vec3(1.0, 0.177, 0.0);
  const vec3 LAVA_COOL_$ID = vec3(0.7, 0.08, 0.0);

  vec2 worldUV = hit.hitPos.xz;
  vec2 flowDir = vec2(-0.35, 0.0);
  float n = smoothstep(0.05, 0.95, noise((worldUV + flowDir * uTime) * 0.73));
  lavaCol = mix(LAVA_COOL_$ID, LAVA_HOT_$ID, n);
#endif

  outEmissive = lavaCol * LAVA_INTENSITY_$ID;
  outEmissiveAlpha = 1.0;
  outColor = LAVA_COLOR_$ID * 0.02;
}

void colorSolidVoxel_$ID(
  VoxelHit_$ID hit,
  vec3 viewDir,
  float tEnter,
  out vec3 outColor,
  out vec3 outEmissive,
  out float outEmissiveAlpha
) {
  outEmissive = vec3(0.0);
  outEmissiveAlpha = 0.0;

  vec3 surfColor = SURFACE_COLOR_$ID;
  vec3 lightNormal = hit.normal;
  float crackLava = 0.0;

  // ---- Plate grid + bevel (all quality levels) ----

  // Analytic pixel footprint in voxel-space on this face.
  float angPx = 2.0 * abs(uProjectionMatrixInverse[1][1])
               / float(textureSize(uSceneDepth, 0).y);
  float cosAngle = max(abs(dot(viewDir, hit.normal)), 0.01);
  float pxVoxel = angPx * tEnter / (VOXEL_SIZE_$ID * cosAngle);

  // Face-aligned continuous UV + tangent frame.
  vec2 surfUV;
  vec3 tangent, bitangent;
  if (abs(hit.normal.y) > 0.5) {
    surfUV = hit.hitPos.xz;
    tangent = vec3(1.0, 0.0, 0.0);
    bitangent = vec3(0.0, 0.0, 1.0);
  } else if (abs(hit.normal.x) > 0.5) {
    surfUV = hit.hitPos.zy;
    tangent = vec3(0.0, 0.0, 1.0);
    bitangent = vec3(0.0, 1.0, 0.0);
  } else {
    surfUV = hit.hitPos.xy;
    tangent = vec3(1.0, 0.0, 0.0);
    bitangent = vec3(0.0, 1.0, 0.0);
  }

  // Per-cell variation of plate size and bevel thickness.
  ivec2 texCellIx, texLocalXZ;
  cellDecompose_$ID(hit.cell.x, hit.cell.z, texCellIx, texLocalXZ);
  float hPlateSize = hash(vec2(texCellIx) + vec2(157.3, 113.7));
  float hBevelSize = hash(vec2(texCellIx) + vec2(173.1, 131.9));

  const float BEVEL_NORMAL_STRENGTH_$ID = 0.45;
  vec2 plateScale = vec2(0.1, 0.1) * (0.7 + 0.6 * hPlateSize);
  float bevelWidth = 0.01 * (0.5 + 1.0 * hBevelSize);

  float pxPlate = pxVoxel * max(plateScale.x, plateScale.y);
  float aaW = max(bevelWidth, pxPlate);

  vec2 scaled = surfUV * plateScale;
  float row = floor(scaled.y);
  scaled.x += step(1.0, mod(row, 2.0)) * 0.5;
  vec2 plateCell = floor(scaled);
  vec2 plateLocal = fract(scaled);

  // Beveled edges — SDF from plate border, AA-widened.
  vec2 edgeDist = 0.5 - abs(plateLocal - 0.5);
  float edgeSDF = min(edgeDist.x, edgeDist.y);
  float bevel = smoothstep(0.0, aaW, edgeSDF);

  // Normal perturbation from bevel (fade out when AA dominates).
  float bevelSharpness = clamp(bevelWidth / aaW, 0.0, 1.0);
  vec2 bevelGrad = vec2(
    smoothstep(aaW, 0.0, edgeDist.x) * sign(plateLocal.x - 0.5),
    smoothstep(aaW, 0.0, edgeDist.y) * sign(plateLocal.y - 0.5)
  );
  lightNormal = normalize(
    hit.normal + (tangent * bevelGrad.x + bitangent * bevelGrad.y)
               * BEVEL_NORMAL_STRENGTH_$ID * bevelSharpness
  );

  surfColor *= bevel;

  // ---- Crack lava: lava seeping up through plate grooves on wall faces ----
  if (abs(hit.normal.y) < 0.5) {
    float grooveDepth = 1.0 - bevel;
    if (grooveDepth > 0.01) {
      float crackCeilingH = hash(vec2(texCellIx) + vec2(7.3, 13.1));
      float ceiling = float(VOLUME_DEPTH_$ID - 2) - floor(crackCeilingH * 10.0);

      // Slow breathing cycle (~50s) with spatial wave along -X+Z diagonal.
      float spatialPhase = dot(hit.hitPos.xz, vec2(-1.0, 1.0)) * 0.015;
      float breathRaw = 0.5 + 0.5 * sin(uTime * 0.1257 + spatialPhase);
      float breathBias = mix(-0.8, 0.3, breathRaw);
      float fillNoise = noise(hit.hitPos.xz * 0.08 + vec2(53.7, 71.3));
      float biasedFill = clamp(fillNoise + breathBias, 0.0, 1.0);
      float lavaFloor = float(VOLUME_DEPTH_$ID - TRENCH_DEPTH_$ID);
      float fillHeight = mix(lavaFloor, ceiling - 1.0, biasedFill);

      float fillAA = max(0.5, pxVoxel / cosAngle);
      float inFill = smoothstep(fillHeight + fillAA, fillHeight - fillAA, hit.hitPos.y);
      crackLava = grooveDepth * inFill;
    }
  }

#if VOX_LAVA_QUALITY >= 1
  // ---- High-quality extras: per-plate variation, scratches, grime ----
  float hBright  = hash(plateCell + vec2(91.3, 47.7));
  float hScratch = hash(vec2(hit.cell));

  float plateBright = 0.95 + 0.35 * hBright;

  float scratchAxis = mix(surfUV.x, surfUV.y, step(hScratch, 0.5));
  float scratches = noise(scratchAxis * 12.0 + hScratch * 200.0);
  scratches = 0.85 + 0.25 * scratches;

  float grime = noise(surfUV * 2.5 + vec2(3.7, 11.3));
  grime = 0.8 + 0.2 * grime;

  surfColor *= plateBright * scratches * grime;
#endif

  // Darken wall faces.
  if (abs(hit.normal.y) < 0.5) {
    surfColor *= 0.4;
  }

  // Lighting with (possibly perturbed) normal.
  float ndotl = max(dot(lightNormal, LIGHT_DIR_$ID), 0.0);
  float lighting = ndotl * LIGHT_INTENSITY_$ID + AMBIENT_$ID;

  outColor = surfColor * lighting;

  // ---- Pseudo-GI from lava — purely depth-based ----
  const float GI_INTENSITY = 0.25;
  const float GI_Y_START   = 4.0;   // depth from top where glow begins
  const float GI_Y_END     = 20.0;  // depth from top where glow is max

  float yDepth = float(VOLUME_DEPTH_$ID - 1) - hit.hitPos.y;
  float giStrength = GI_INTENSITY * smoothstep(GI_Y_START, GI_Y_END, yDepth);

  // tone down GI by 50% if we're at the top of a voxel (normal is (0,1,0))
  if (hit.normal.y > 0.5) {
    giStrength *= 0.5;
  }

  if (giStrength > 0.001) {
    // Bevel modulates GI on walls so plate grooves show as dark lines in the glow.
    float giBevel = (abs(hit.normal.y) < 0.5) ? mix(0.4, 1.0, bevel) : 1.0;
    outEmissive = LAVA_COLOR_$ID * LAVA_INTENSITY_$ID * giStrength * giBevel;
    outEmissiveAlpha = giStrength * giBevel;
  }

  // Crack lava in wall grooves — adds emissive on top of GI.
  if (crackLava > 0.001) {
    vec3 crackEmissive = LAVA_COLOR_$ID * LAVA_INTENSITY_$ID * crackLava * 2.1;
    outEmissive = max(outEmissive, crackEmissive);
    outEmissiveAlpha = max(outEmissiveAlpha, crackLava);
    // Darken the surface in the cracks so the lava reads clearly.
    outColor *= 1.0 - crackLava * 0.8;
  }
}

void sampleVoxelGround_$ID(
  vec3 dir,
  out vec3 outColor,
  out vec3 outEmissive,
  out float outAlpha,
  out float outEmissiveAlpha
) {
  outColor = vec3(0.0);
  outEmissive = vec3(0.0);
  outAlpha = 0.0;
  outEmissiveAlpha = 0.0;

  if (dir.y > 0.01) return;

  float negDy = max(-dir.y, 1e-4);

  // Parallax: camera world position offsets the grid.
  vec3 camPos = uCameraWorldMatrix[3].xyz * PARALLAX_SCALE_$ID;
  float effectiveHeight = max(GROUND_HEIGHT_$ID + camPos.y, 10.0);
  float tEnter = effectiveHeight / negDy;

  // Distance fog controls alpha — subsumes horizon fade.
  float fogT = smoothstep(FOG_START_$ID, FOG_END_$ID, tEnter);
  float alpha = 1.0 - fogT;

  if (alpha < 0.001) return;

  // LOD: angular size of one voxel at the entry distance.
  float angularPx = 2.0 * abs(uProjectionMatrixInverse[1][1])
                   / float(textureSize(uSceneDepth, 0).y);
  float voxelAngular = VOXEL_SIZE_$ID / tEnter;
  float lodFactor = 1.0 - smoothstep(0.5 * angularPx, 2.0 * angularPx, voxelAngular);

  // Average-color estimate for LOD.
  float avgNdotL = max(LIGHT_DIR_$ID.y, 0.0);
  float avgLighting = avgNdotL * LIGHT_INTENSITY_$ID + AMBIENT_$ID;
  vec3 avgColor = SURFACE_COLOR_$ID * avgLighting;

  // Fully LOD'd — skip DDA.
  if (lodFactor > 0.99) {
    outColor = avgColor * alpha;
    outAlpha = alpha;
    return;
  }

  // ---- DDA trace ----

  vec3 entryWorld = dir * tEnter;
  vec3 voxelOrigin = vec3(
    (entryWorld.x + camPos.x) / VOXEL_SIZE_$ID,
    float(VOLUME_DEPTH_$ID) - 0.001,
    (entryWorld.z + camPos.z) / VOXEL_SIZE_$ID
  );

  VoxelHit_$ID hit = traceVoxels_$ID(voxelOrigin, dir);

  if (!hit.hit) {
    outColor = avgColor * 0.5 * alpha;
    outAlpha = alpha;
    return;
  }

  vec3 hitColor;
  vec3 hitEmissive = vec3(0.0);
  float hitEmissiveAlpha = 0.0;

  if (hit.cell.y == VOLUME_DEPTH_$ID - TRENCH_DEPTH_$ID - 1) {
    colorLava_$ID(hit, hitColor, hitEmissive, hitEmissiveAlpha);
  } else {
    colorSolidVoxel_$ID(hit, dir, tEnter, hitColor, hitEmissive, hitEmissiveAlpha);
  }

  // Blend with LOD average (emissive fades to zero at LOD).
  vec3 finalColor = mix(hitColor, avgColor, lodFactor);
  vec3 finalEmissive = hitEmissive * (1.0 - lodFactor);
  float finalEmAlpha = hitEmissiveAlpha * (1.0 - lodFactor);

  // Apply fog via alpha.
  float emAlpha = alpha * (1.0 - fogT * FOG_EMISSIVE_ATTEN_$ID);
  outColor = finalColor * alpha;
  outEmissive = finalEmissive * emAlpha;
  outAlpha = alpha;
  outEmissiveAlpha = finalEmAlpha * emAlpha;
}
