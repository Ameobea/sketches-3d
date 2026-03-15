import type * as THREE from 'three';
import type { SceneConfig } from '../scenes';
import { DefaultMoveSpeed } from '../scenes';
import { DefaultGravity, DefaultJumpSpeed } from '../conf';

export interface JumpSimParams {
  /** Gravity magnitude, units/s². ParkourManager default: 30 */
  gravity: number;
  /** Initial vertical velocity when jumping, units/s. ParkourManager default: 12 */
  jumpVelocity: number;
  /** Horizontal movement speed while airborne, units/s. ParkourManager default: 13 */
  inAirSpeed: number;
  /** Physics simulation tick rate in Hz. SceneConfig default: 160 */
  tickRate: number;
  /** Terminal fall speed in units/s. Default: 55 (matches btKinematicCharacterController default) */
  terminalVelocity?: number;
  /**
   * Horizontal lean factor baked into the jump vector, matching collision.ts:
   *   jump(moveDir.x * jumpSpeed * jumpTiltFactor, jumpSpeed, moveDir.z * jumpSpeed * jumpTiltFactor)
   *
   * During the rising phase, stepUp moves along the tilted jump axis rather than straight up,
   * adding (jumpTiltFactor * |moveDir_xz|) * maxJumpHeight of extra horizontal displacement.
   * During the falling phase, stepDown moves along pure vertical — no tilt contribution.
   *
   * Default: 0.18 (matches the hardcoded 0.18 in collision.ts).
   * Set to 0 to disable (e.g. if jumping with no horizontal input).
   */
  jumpTiltFactor?: number;
  /**
   * Magnitude of the movement direction vector when jumping. Affects both the walkDir speed and
   * the jump tilt horizontal contribution.
   *
   * In collision.ts, moveDirection is built additively from camera-relative unit vectors:
   *   - Single key (W/A/S/D): magnitude 1
   *   - Diagonal (W+A etc.):  magnitude √2 ≈ 1.414 — a real speed boost
   *   - With easyModeMovement on: always scaled to √2 regardless of key count
   *
   * Default: √2, because easyModeMovement is always-on (and even without it, a player
   * optimizing for speed on a non-axis-aligned path uses diagonal movement anyway).
   */
  moveDirMagnitude?: number;
  gravityShaping?: {
    /** Gravity multiplier when rising quickly. Default: 1.0 */
    riseMultiplier?: number;
    /** Gravity multiplier near the apex. Default: 1.0 */
    apexMultiplier?: number;
    /** Gravity multiplier when falling quickly. Default: 1.0 */
    fallMultiplier?: number;
    /** |verticalVelocity| (units/s) that defines the center of the apex zone. Default: 3.0 */
    apexThreshold?: number;
    /** Width of smooth transition between zones, units/s. Default: 2.0 */
    kneeWidth?: number;
    /** If true, shaping only applies during jumps (not walk-off-ledge falls). Default: false */
    onlyJumps?: boolean;
  };
}

export interface ArcSample {
  time: number;
  height: number;
}

interface NormalizedShaping {
  riseMultiplier: number;
  apexMultiplier: number;
  fallMultiplier: number;
  apexThreshold: number;
  kneeWidth: number;
  onlyJumps: boolean;
}

function normalizeShaping(gs: JumpSimParams['gravityShaping']): NormalizedShaping {
  return {
    riseMultiplier: gs?.riseMultiplier ?? 1.0,
    apexMultiplier: gs?.apexMultiplier ?? 1.0,
    fallMultiplier: gs?.fallMultiplier ?? 1.0,
    apexThreshold: gs?.apexThreshold ?? 3.0,
    kneeWidth: gs?.kneeWidth ?? 2.0,
    onlyJumps: gs?.onlyJumps ?? false,
  };
}

// Port of btKinematicCharacterController::computeShapedGravity.
// isJumping should be true during a jump arc simulation.
function computeShapedGravity(
  verticalVelocity: number,
  gravity: number,
  shaping: NormalizedShaping,
  isJumping: boolean
): number {
  if (shaping.onlyJumps && !isJumping) {
    return gravity;
  }
  if (shaping.riseMultiplier === 1.0 && shaping.apexMultiplier === 1.0 && shaping.fallMultiplier === 1.0) {
    return gravity;
  }

  const absV = Math.abs(verticalVelocity);
  const threshold = shaping.apexThreshold;
  const halfKnee = shaping.kneeWidth * 0.5;

  const smoothstep = (edge0: number, edge1: number, x: number): number => {
    if (edge1 <= edge0) return x >= edge0 ? 1.0 : 0.0;
    const t = Math.max(0.0, Math.min(1.0, (x - edge0) / (edge1 - edge0)));
    return t * t * (3.0 - 2.0 * t);
  };

  const outerMultiplier = verticalVelocity >= 0.0 ? shaping.riseMultiplier : shaping.fallMultiplier;
  const blend = smoothstep(threshold - halfKnee, threshold + halfKnee, absV);
  const multiplier = shaping.apexMultiplier + (outerMultiplier - shaping.apexMultiplier) * blend;
  return gravity * multiplier;
}

/**
 * Simulates the vertical arc of a single jump starting at height 0 with vertical velocity
 * `jumpVelocity`. Runs for 10 simulated seconds at the given tick rate, matching the
 * btKinematicCharacterController integration order (gravity applied before offset each tick).
 *
 * The arc is not truncated at landing height 0 — it continues below to support computing
 * air time to platforms lower than the launch point.
 */
export function simulateJumpArc({
  gravity,
  jumpVelocity,
  tickRate,
  gravityShaping,
  terminalVelocity = 55,
}: JumpSimParams): ArcSample[] {
  const shaping = normalizeShaping(gravityShaping);
  const dt = 1 / tickRate;
  const maxTicks = Math.ceil(10 * tickRate);

  const samples: ArcSample[] = [{ time: 0, height: 0 }];
  let verticalVelocity = jumpVelocity;
  let height = 0;
  let time = 0;

  for (let i = 0; i < maxTicks; i++) {
    // Match C++ playerStep order: apply gravity to velocity first, then compute offset
    verticalVelocity -= computeShapedGravity(verticalVelocity, gravity, shaping, true) * dt;
    // Match C++ terminal velocity clamp (only caps downward/falling velocity)
    if (verticalVelocity < 0 && Math.abs(verticalVelocity) > terminalVelocity) {
      verticalVelocity = -terminalVelocity;
    }
    height += verticalVelocity * dt;
    time += dt;
    samples.push({ time, height });
  }

  return samples;
}

/**
 * Returns the air time (seconds) for the player to reach height `deltaH` (relative to launch),
 * or null if `deltaH` is above the arc apex (unreachable).
 *
 * Always returns the LAST crossing of `deltaH` in the arc, which corresponds to the descent
 * phase and gives maximum horizontal range.
 */
export function getAirTime(arc: ArcSample[], maxArcHeight: number, deltaH: number): number | null {
  if (deltaH > maxArcHeight) return null;

  // Search backwards for last crossing of deltaH
  for (let i = arc.length - 1; i > 0; i--) {
    const s0 = arc[i - 1];
    const s1 = arc[i];
    // Product <= 0 means s0 and s1 are on opposite sides of deltaH, or one equals it
    if ((s0.height - deltaH) * (s1.height - deltaH) <= 0) {
      const dh = s1.height - s0.height;
      if (Math.abs(dh) < 1e-10) return s0.time;
      const frac = (deltaH - s0.height) / dh;
      return s0.time + frac * (s1.time - s0.time);
    }
  }

  return null;
}

const DEFAULT_TERMINAL_VELOCITY = 55;
const DEFAULT_SIMULATION_TICK_RATE_HZ = 160;

/**
 * Builds a `JumpSimParams` from a `SceneConfig` (as returned by `ParkourManager.buildSceneConfig()`),
 * pulling gravity, jump velocity, in-air speed, tick rate, terminal velocity, and gravity shaping
 * from the same fields the physics engine reads at runtime.
 *
 * Any field not set in the `SceneConfig` falls back to the engine defaults.
 */
export function jumpSimParamsFromSceneConfig(sceneConfig: SceneConfig): JumpSimParams {
  return {
    gravity: sceneConfig.gravity ?? DefaultGravity,
    jumpVelocity: sceneConfig.player?.jumpVelocity ?? DefaultJumpSpeed,
    inAirSpeed: (sceneConfig.player?.moveSpeed ?? DefaultMoveSpeed).inAir,
    tickRate: sceneConfig.simulationTickRate ?? DEFAULT_SIMULATION_TICK_RATE_HZ,
    terminalVelocity: sceneConfig.player?.terminalVelocity ?? DEFAULT_TERMINAL_VELOCITY,
    gravityShaping: sceneConfig.gravityShaping,
  };
}

/**
 * Generates platform positions along a spline for parkour jumping.
 *
 * Places platforms as far apart as a player can reach in a single jump, assuming:
 * - Instant re-jump on landing (jump key held)
 * - Perfect aim directly toward the next platform
 * - Full in-air speed maintained throughout each jump
 *
 * Uses arc-length parameterization of the spline (`getPointAt`), so the binary search
 * operates in terms of distance rather than raw curve parameter.
 *
 * @param spline    Path along which platforms are placed. THREE.Curve<Vector3> or any subclass.
 * @param params    Player physics params.
 * @param fudgeFactor  Multiplier on jump range. < 1 = closer platforms, > 1 = farther. Default 1.
 * @returns         Array of THREE.Vector3 platform top-surface centers in Y-up world space.
 *                  Always includes the spline start and end points.
 */
export function generateParkourPlatforms(
  spline: THREE.Curve<THREE.Vector3>,
  params: JumpSimParams,
  fudgeFactor = 1.0
): THREE.Vector3[] {
  const arc = simulateJumpArc(params);
  const maxArcHeight = arc.reduce((m, s) => Math.max(m, s.height), -Infinity);

  const moveDirMag = params.moveDirMagnitude ?? Math.SQRT2;
  const jumpTilt = params.jumpTiltFactor ?? 0.18;
  // Horizontal displacement added during the rising phase from the tilted jump axis.
  // Derivation: ratio of horizontal-to-vertical movement per tick = jumpTilt * moveDirMag,
  // so total = jumpTilt * moveDirMag * maxHeight (accumulated over entire rise regardless of deltaH).
  const jumpTiltHorizBonus = jumpTilt * moveDirMag * maxArcHeight;
  // Effective horizontal speed including moveDirMag scale
  const effectiveAirSpeed = params.inAirSpeed * moveDirMag;

  const platforms: THREE.Vector3[] = [spline.getPointAt(0)];

  const EPSILON = 1e-5;
  const MAX_BINARY_ITERS = 64;

  const isReachable = (current: THREE.Vector3, u: number): boolean => {
    const candidate = spline.getPointAt(u);
    // Y is up in Three.js world space
    const deltaH = candidate.y - current.y;
    const airTime = getAirTime(arc, maxArcHeight, deltaH);
    if (airTime === null) return false;

    // Total horizontal range = walkDir contribution + jump axis tilt contribution.
    // The tilt bonus is constant regardless of deltaH (always accumulated over the full rise).
    const maxHorizRange = (effectiveAirSpeed * airTime + jumpTiltHorizBonus) * fudgeFactor;
    const horizDist = Math.sqrt((candidate.x - current.x) ** 2 + (candidate.z - current.z) ** 2);
    return horizDist <= maxHorizRange;
  };

  // Compute flat-ground max jump range to use as the minimum gap threshold.
  // If the final platform would be closer than this to the previous one, skip it.
  const flatAirTime = getAirTime(arc, maxArcHeight, 0)!;
  const fullJumpSpan = (effectiveAirSpeed * flatAirTime + jumpTiltHorizBonus) * fudgeFactor;

  let u = 0;

  while (u < 1 - EPSILON) {
    const current = platforms[platforms.length - 1];

    // Sanity check: ensure we can make at least minimal forward progress
    const minStepU = 0.001;
    const checkU = Math.min(u + minStepU, 1);
    if (checkU > u && !isReachable(current, checkU)) {
      console.warn(
        `generateParkourPlatforms: cannot make progress at spline u=${u.toFixed(4)}.` +
          ` Position: (${current.x.toFixed(2)}, ${current.y.toFixed(2)}, ${current.z.toFixed(2)}).` +
          ` Consider reducing fudgeFactor or adjusting the spline.`
      );
      break;
    }

    // Binary search: find farthest reachable u in (current_u, 1]
    let lo = u;
    let hi = 1.0;
    for (let iter = 0; iter < MAX_BINARY_ITERS; iter++) {
      const mid = (lo + hi) / 2;
      if (isReachable(current, mid)) {
        lo = mid;
      } else {
        hi = mid;
      }
      if (hi - lo < EPSILON) break;
    }

    u = lo;
    const candidate = spline.getPointAt(u);

    // Skip the final platform if it's less than a full jump span from the last placed one.
    // This avoids placing two platforms awkwardly close together at the end of the path.
    const dx = candidate.x - current.x;
    const dz = candidate.z - current.z;
    const dist = Math.sqrt(dx * dx + dz * dz);
    if (u >= 1 - EPSILON && dist < fullJumpSpan) {
      break;
    }

    platforms.push(candidate);
  }

  return platforms;
}
