// Shared discrete-building probe. Every building sub-layer (silhouette,
// silhouette-attenuator, windows) calls the same `probeBuilding()` so they
// agree on which fragments are inside a building body and on the window-grid
// coordinates within that body.
//
// Depends on prelude's `TWO_PI` and on `hash(vec2)` from noise.frag — include
// noise.frag, prelude, then this file, then the layer entry shader.

uniform float uBuildingCount;
uniform float uBuildingPresence;
uniform float uBuildingGap;
uniform float uBuildingMinHeight;
uniform float uBuildingMaxHeight;
uniform float uFloorsMin;
uniform float uFloorsMax;
uniform float uWindowsMin;
uniform float uWindowsMax;
uniform float uMaxFloorStride;
uniform float uMaxWindowStride;
uniform float uLitFractionMin;
uniform float uLitFractionMax;
uniform float uGroundElev;

struct BuildingHit {
  bool hasBody;
  vec2 bCell;        // per-building hash cell (slot index)
  float localX;      // [0, 1] across the building's body width
  float localY;      // [0, 1] from ground to top of this building
  float floorIdx;    // integer floor index within the building
  float windowIdx;   // integer window-column index within the building
  vec2 cellLocal;    // [0, 1]² within the current (floor × window) cell
  float floorCount;
  float windowCount;
  float floorStride;
  float windowStride;
  float litFrac;
  float colorH;      // per-building color-mix hash in [0, 1]
};

BuildingHit probeBuilding(float elev, float azimuth) {
  BuildingHit hit;
  hit.hasBody = false;
  hit.bCell = vec2(0.0);
  hit.localX = 0.0;
  hit.localY = 0.0;
  hit.floorIdx = 0.0;
  hit.windowIdx = 0.0;
  hit.cellLocal = vec2(0.0);
  hit.floorCount = 1.0;
  hit.windowCount = 1.0;
  hit.floorStride = 1.0;
  hit.windowStride = 1.0;
  hit.litFrac = 0.0;
  hit.colorH = 0.0;

  // Static cull: anything below the building base or above the tallest possible
  // top exits before we touch any hashes. Without this, fragments well above
  // the cityscape still pay for `presentH`, `bodyWidth`, and `heightH` lookups.
  if (elev < uGroundElev || elev > uGroundElev + uBuildingMaxHeight) {
    return hit;
  }

  float az01 = azimuth / TWO_PI + 0.5;

  // Slot selection: each slot is one candidate building around the horizon.
  float slotF = az01 * uBuildingCount;
  float slotIdx = floor(slotF);
  float slotLocal = fract(slotF);
  vec2 bCell = vec2(slotIdx, 0.0);

  // Per-slot presence — fraction of slots that actually host a building.
  float presentH = hash(bCell + vec2(11.1, 13.3));
  if (presentH > uBuildingPresence) {
    return hit;
  }

  // Gap on each side of the building within its slot so adjacent towers don't
  // butt together visually.
  float halfGap = clamp(uBuildingGap, 0.0, 0.98) * 0.5;
  float bodyWidth = max(1.0 - 2.0 * halfGap, 1e-4);
  if (slotLocal < halfGap || slotLocal > 1.0 - halfGap) {
    return hit;
  }
  float localX = (slotLocal - halfGap) / bodyWidth;

  // Per-building height → top elevation check.
  float heightH = hash(bCell + vec2(17.3, 23.5));
  float buildingHeight = mix(uBuildingMinHeight, uBuildingMaxHeight, heightH);
  float topElev = uGroundElev + buildingHeight;
  if (elev > topElev) {
    return hit;
  }
  float localY = (elev - uGroundElev) / max(buildingHeight, 1e-4);

  // Per-building floor/window counts, strides, lit fraction, color bias.
  float floorsH = hash(bCell + vec2(2.3, 5.7));
  float windowsH = hash(bCell + vec2(8.9, 4.1));
  float floorCount = max(1.0, floor(mix(uFloorsMin, uFloorsMax, floorsH) + 0.5));
  float windowCount = max(1.0, floor(mix(uWindowsMin, uWindowsMax, windowsH) + 0.5));

  float floorStrideH = hash(bCell + vec2(7.7, 9.3));
  float windowStrideH = hash(bCell + vec2(5.1, 2.9));
  float floorStride = max(1.0, floor(mix(1.0, uMaxFloorStride, floorStrideH) + 0.5));
  float windowStride = max(1.0, floor(mix(1.0, uMaxWindowStride, windowStrideH) + 0.5));

  float litH = hash(bCell + vec2(3.1, 19.7));
  float litFrac = mix(uLitFractionMin, uLitFractionMax, litH);

  float colorH = hash(bCell + vec2(29.3, 31.7));

  // Grid cell within the building body.
  float floorF = localY * floorCount;
  float windowF = localX * windowCount;
  float floorIdx = floor(floorF);
  float windowIdx = floor(windowF);
  vec2 cellLocal = vec2(fract(windowF), fract(floorF));

  hit.hasBody = true;
  hit.bCell = bCell;
  hit.localX = localX;
  hit.localY = localY;
  hit.floorIdx = floorIdx;
  hit.windowIdx = windowIdx;
  hit.cellLocal = cellLocal;
  hit.floorCount = floorCount;
  hit.windowCount = windowCount;
  hit.floorStride = floorStride;
  hit.windowStride = windowStride;
  hit.litFrac = litFrac;
  hit.colorH = colorH;
  return hit;
}
