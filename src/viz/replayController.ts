import * as THREE from 'three';

import {
  type FlightPlayer,
  type ReplayHeaderMismatch,
  type ReplayValidationConfig,
  type SubtickPhysicsSnapshot,
  validateReplayHeader,
} from './flightRecorder.js';
import type { BulletPhysics, SubtickInputState } from './collision.js';

const MAX_SUBSTEPS_PER_FRAME = 120;

// The deterministic replay path depends on these fields matching the live
// controller configuration; if they differ, playback is not authoritative.
const FATAL_REPLAY_HEADER_FIELDS = new Set<keyof ReplayValidationConfig>([
  'tickRateHz',
  'gravity',
  'jumpSpeed',
  'moveSpeedGround',
  'moveSpeedAir',
  'colliderHeight',
  'colliderRadius',
  'extVelAirDamping',
  'extVelGroundDamping',
  'gravityShapeRiseMult',
  'gravityShapeApexMult',
  'gravityShapeFallMult',
  'gravityShapeApexThreshold',
  'gravityShapeKneeWidth',
  'gravityShapeOnlyJumps',
  'stepHeight',
  'terminalVelocity',
  'maxSlopeRadians',
  'maxPenetrationDepth',
  'coyoteTimeSeconds',
  'minJumpDelaySeconds',
  'easyModeMovement',
  'colliderShape',
  'dashEnabled',
  'dashMagnitude',
  'minDashDelaySeconds',
  'dashUseExternalVelocity',
]);

/** A single field mismatch detected during replay validation. */
interface ReplayFieldMismatch {
  field: string;
  recorded: number | boolean;
  replayed: number | boolean;
  absDiff?: number;
}

/** First-divergence report from deterministic replay validation. */
interface ReplayDivergence {
  subtickIndex: number;
  physicsTime: number;
  mismatches: ReplayFieldMismatch[];
}

/**
 * Owns all deterministic replay state and logic.  Drives the physics simulation
 * through `ReplayPhysicsHost.advanceOneSubtick` using recorded per-subtick
 * input rather than live input.
 */
export class ReplayController {
  private readonly physics: BulletPhysics;

  private player: FlightPlayer | null = null;
  private subtickIndex = 0;
  private active = false;
  private firstDivergence: ReplayDivergence | null = null;
  private maxPosDivergence = 0;
  private maxPosDivergenceSubtick = 0;
  private startCbs: (() => void)[] = [];

  constructor(physics: BulletPhysics) {
    this.physics = physics;
  }

  public get isActive(): boolean {
    return this.active;
  }

  public registerStartCb = (cb: () => void) => this.startCbs.push(cb);

  public destroy = () => {
    if (this.player) {
      this.player.destroy();
      this.player = null;
    }
  };

  public start = (player: FlightPlayer): void => {
    this.destroy();
    const header = player.getHeader();
    const mismatches = validateReplayHeader(header, this.physics.buildPhysicsSimConfig());
    const formatHeaderMismatch = (m: ReplayHeaderMismatch) => {
      const diff = m.absDiff !== undefined ? ` (Δ${m.absDiff.toExponential(4)})` : '';
      return `${m.field}: replay=${m.recorded}, current=${m.current}${diff}`;
    };
    const fatalMismatches = mismatches.filter(m => FATAL_REPLAY_HEADER_FIELDS.has(m.field));
    if (fatalMismatches.length > 0) {
      player.destroy();
      throw new Error(
        `Cannot replay: incompatible physics configuration\n` +
          fatalMismatches.map(m => `  ${formatHeaderMismatch(m)}`).join('\n')
      );
    }
    if (mismatches.length > 0) {
      console.warn(
        `Replay header mismatches (${mismatches.length}):\n` +
          mismatches.map(m => `  ${formatHeaderMismatch(m)}`).join('\n')
      );
    }

    // Teleport player to the recorded spawn position so that the initial
    // ground-contact state matches what was present during recording.
    const spawnPosStr = player.getMetadataString('spawn_pos');
    const spawnRotStr = player.getMetadataString('spawn_rot');
    if (spawnPosStr) {
      const [sx, sy, sz] = spawnPosStr.split(',').map(Number);
      const rot = spawnRotStr ? spawnRotStr.split(',').map(Number) : undefined;
      this.physics.teleportPlayer([sx, sy, sz], rot ? [rot[0], rot[1], rot[2]] : undefined);
    }

    // Clear all dynamic state (velocities, flags, floor refs, cooldowns, timing)
    // to match a freshly-loaded level.
    this.physics.reset();
    this.physics.resetDashStateForNewRun();

    this.physics.assertInitialState();

    this.player = player;
    this.subtickIndex = 0;
    this.active = true;
    this.firstDivergence = null;
    this.maxPosDivergence = 0;
    this.maxPosDivergenceSubtick = 0;
    this.physics.viz.controlState.movementEnabled = false;
    for (const cb of this.startCbs) {
      cb();
    }
  };

  public stop = (): void => {
    if (this.firstDivergence) {
      const d = this.firstDivergence;
      const fields = d.mismatches.map(m => {
        const diff = m.absDiff !== undefined ? ` (Δ${m.absDiff.toExponential(4)})` : '';
        return `    ${m.field}: recorded=${m.recorded}, replayed=${m.replayed}${diff}`;
      });
      console.warn(
        `[replay-validation] Summary: first divergence at subtick ${d.subtickIndex} ` +
          `(t=${d.physicsTime.toFixed(4)}s), ` +
          `max pos drift=${this.maxPosDivergence.toExponential(4)} at subtick ${this.maxPosDivergenceSubtick}\n` +
          `  First divergence fields:\n${fields.join('\n')}`
      );
    }

    this.active = false;
    this.destroy();
    this.physics.viz.controlState.movementEnabled = true;
  };

  private validateSubtickAgainstSnapshot = (
    subtickIndex: number,
    recorded: SubtickPhysicsSnapshot
  ): ReplayDivergence | null => {
    const { physics } = this;
    physics.playerController.packState(physics.packStateBufPtr);
    const heap = physics.Ammo.HEAPF32;
    const base = physics.packStateBufPtr / 4;

    // Read current state from Ammo heap (same layout as packState: 10 floats)
    const curPosX = heap[base];
    const curPosY = heap[base + 1];
    const curPosZ = heap[base + 2];
    const curExtVelX = heap[base + 3];
    const curExtVelY = heap[base + 4];
    const curExtVelZ = heap[base + 5];
    const curVertVel = heap[base + 6];
    const curVertOffset = heap[base + 7];

    // Flags are bitcast u32 at [8]
    const flagsU32 = new DataView(heap.buffer, physics.packStateBufPtr + 32, 4).getUint32(0, true);
    const curOnGround = (flagsU32 & 1) !== 0;
    const curIsJumping = (flagsU32 & 2) !== 0;

    // Floor user index is bitcast i32 at [9]
    const curFloorIdx = new DataView(heap.buffer, physics.packStateBufPtr + 36, 4).getInt32(0, true);

    const mismatches: ReplayFieldMismatch[] = [];
    const TOL = 1e-4;

    const checkFloat = (field: string, rec: number, cur: number) => {
      const diff = Math.abs(rec - cur);
      if (diff > TOL) {
        mismatches.push({ field, recorded: rec, replayed: cur, absDiff: diff });
      }
    };
    const checkBool = (field: string, rec: boolean, cur: boolean) => {
      if (rec !== cur) {
        mismatches.push({ field, recorded: rec, replayed: cur });
      }
    };
    const checkInt = (field: string, rec: number, cur: number) => {
      if (rec !== cur) {
        mismatches.push({ field, recorded: rec, replayed: cur, absDiff: Math.abs(rec - cur) });
      }
    };

    checkFloat('pos.x', recorded.pos[0], curPosX);
    checkFloat('pos.y', recorded.pos[1], curPosY);
    checkFloat('pos.z', recorded.pos[2], curPosZ);
    checkFloat('extVel.x', recorded.externalVel[0], curExtVelX);
    checkFloat('extVel.y', recorded.externalVel[1], curExtVelY);
    checkFloat('extVel.z', recorded.externalVel[2], curExtVelZ);
    checkFloat('verticalVel', recorded.verticalVel, curVertVel);
    checkFloat('verticalOffset', recorded.verticalOffset, curVertOffset);
    checkBool('onGround', recorded.onGround, curOnGround);
    checkBool('isJumping', recorded.isJumping, curIsJumping);
    checkInt('floorUserIndex', recorded.floorUserIndex, curFloorIdx);

    if (mismatches.length === 0) {
      return null;
    } else {
      return { subtickIndex, physicsTime: physics.getPhysicsTime(), mismatches };
    }
  };

  /**
   * Deterministic replay: drives the game through the same gameplay logic as live
   * play, using recorded per-subtick input (key flags + camera angles) instead of
   * live input.  Jump/dash decisions are made by the same code paths as live play
   * rather than being force-injected from recorded events.
   *
   * After each substep, validates the resulting physics state against the recorded
   * snapshot and tracks the first divergence and maximum position drift.
   */
  public tick = (tDiffSeconds: number): THREE.Vector3 => {
    const { physics } = this;
    const player = this.player!;
    physics.playerController.resetForcedRotation();

    const fixedTimeStep = 1 / physics.simulationTickRate;

    const numSubSteps = physics.computeSubstepCount(tDiffSeconds, fixedTimeStep, MAX_SUBSTEPS_PER_FRAME);
    let prevOnGround = physics.playerController.onGround();
    for (let i = 0; i < numSubSteps; i++) {
      if (this.subtickIndex >= player.subtickCount) {
        this.stop();
        break;
      }

      // Build input state from the recorded subtick data
      const subtick = player.getSubtick(this.subtickIndex);
      const input: SubtickInputState = {
        keyFlags: subtick.keyFlags,
        phi: subtick.phi,
        theta: subtick.theta,
        zoomDistance: subtick.zoomDistance,
        // Recorded key flags already reflect the effective controller input
        // consumed during live play, including any JS-side gating.
        movementEnabled: true,
      };

      const result = physics.advanceOneSubtick(input, fixedTimeStep, prevOnGround);

      const recorded = player.getSubtickPhysicsSnapshot(this.subtickIndex);
      const divergence = this.validateSubtickAgainstSnapshot(this.subtickIndex, recorded);
      if (divergence) {
        if (!this.firstDivergence) {
          this.firstDivergence = divergence;
          console.warn(
            `[replay-validation] First divergence at subtick ${divergence.subtickIndex} ` +
              `(t=${divergence.physicsTime.toFixed(4)}s):`,
            divergence.mismatches
          );
        }

        const posMismatch = divergence.mismatches.find(m => m.field.startsWith('pos.'));
        if (posMismatch && posMismatch.absDiff !== undefined) {
          const pos = physics.playerController.getPosition();
          const dx = recorded.pos[0] - pos.x();
          const dy = recorded.pos[1] - pos.y();
          const dz = recorded.pos[2] - pos.z();
          const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
          if (dist > this.maxPosDivergence) {
            this.maxPosDivergence = dist;
            this.maxPosDivergenceSubtick = this.subtickIndex;
          }
        }
      }

      prevOnGround = result.nowOnGround;
      this.subtickIndex++;
    }
    physics.syncPhysicsTickerVisuals();

    const newPlayerPos = physics.playerGhostObject.getWorldTransform().getOrigin();
    return new THREE.Vector3(newPlayerPos.x(), newPlayerPos.y(), newPlayerPos.z());
  };
}
