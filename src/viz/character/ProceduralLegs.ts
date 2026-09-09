import * as THREE from 'three';

import { type CharacterSkeleton, restPosition } from './skeleton';

/** All lengths are model units (the skinned mesh's own space, before fitting to the collider). */
export interface ProceduralLegsConf {
  stride: number;
  lift: number;
  /** Fraction of the cycle a foot is planted. */
  duty: number;
  /** Leg extension at the stance ends as a fraction of leg length; sets the walking crouch. */
  reach: number;
  idleCrouch: number;
  minCycleSeconds: number;
  maxCycleSeconds: number;
  tuck: number;
  airBend: number;
  jumpReleaseTau: number;
  /** How quickly feet drift under the hips once movement stops. */
  settleTau: number;
  crouchInTau: number;
  crouchOutTau: number;
  /** Extra crouch on touching down: full depth over `landAttackTau`, recovery over `landRecoverTau`. */
  landCrouch: number;
  landAttackTau: number;
  landRecoverTau: number;
  /** Intended walk speed (world units/s) below which the body counts as standing. */
  minWalkSpeed: number;
  /** Cadence when input pushes but the body doesn't move (into a wall), as a fraction of input speed. */
  wallCadence: number;
  /** Per-leg phase offsets in bone-name order; defaults to evenly spaced. */
  offsets?: number[];
}

export interface LegsInput {
  dt: number;
  /** Body velocity in world units/s, from the collider's actual motion. */
  bodyVel: THREE.Vector3;
  /** Intended walk speed (input-driven), world units/s; gates walking and drives cadence at a wall. */
  walkSpeed: number;
  onGround: boolean;
  /** A jump event fired since the last update, even if the body was airborne at both frame edges. */
  jumped: boolean;
  /** Likewise for touching down; frame-edge detection is used when omitted. */
  landed?: boolean;
  /**
   * Ground height under a world-space point, cast downward from `fromY` up to `maxDist`; `null`
   * when nothing is there.  Without it feet use the flat floor at the collider bottom.
   */
  probeGround?: (x: number, fromY: number, z: number, maxDist: number) => number | null;
}

interface Leg {
  thigh: THREE.Bone;
  shin: THREE.Bone;
  hipRest: THREE.Vector3;
  thighLen: number;
  shinLen: number;
  outward: THREE.Vector3;
  thighRestDir: THREE.Vector3;
  shinRestDir: THREE.Vector3;
  thighRestQ: THREE.Quaternion;
  shinRestQ: THREE.Quaternion;
  coxaRestQInv: THREE.Quaternion;
  offset: number;
  /** World-space foot target and, while planted, its fixed world position. */
  target: THREE.Vector3;
  liftoff: THREE.Vector3;
  inSwing: boolean;
}

/** Rays reach just past full leg extension; deeper hits are clamped by the IK anyway. */
const ProbeMargin = 1.05;

const expDecay = (dt: number, tau: number) => 1 - Math.exp(-dt / tau);

const _v = new THREE.Vector3();
const _horizVel = new THREE.Vector3();
const _moveDir = new THREE.Vector3();
const _underHip = new THREE.Vector3();
const _point = new THREE.Vector3();
const _hip = new THREE.Vector3();
const _foot = new THREE.Vector3();
const _toFoot = new THREE.Vector3();
const _side = new THREE.Vector3();
const _thighDir = new THREE.Vector3();
const _knee = new THREE.Vector3();
const _shinDir = new THREE.Vector3();
const _q = new THREE.Quaternion();
const _q2 = new THREE.Quaternion();
const _pivotInv = new THREE.Matrix4();

/**
 * Drives a legged skeleton without clips: feet are planted in world space and released on a
 * phase-offset schedule, swings arc to where the body will be, and each leg is solved with
 * analytic two-bone IK.  Bone orientations are pure swings from the rest pose, so segments never
 * twist.  Works in the model's own space, so construct it while the skeleton's world transform
 * is still identity; `pivot` is the model's parent at runtime, carrying the fit scale, yaw and
 * position.
 */
export class ProceduralLegs {
  private readonly legs: Leg[] = [];
  private readonly rootBone: THREE.Bone;
  private readonly rootRestY: number;
  private readonly floorRestY: number;
  private readonly walkCrouch: number;
  private readonly legLen: number;
  private phase = 0;
  private crouch = 0;
  private landTarget = 0;
  private landDip = 0;
  private totalCrouch = 0;
  private airLift = 0;
  private wasOnGround = true;
  private needsReset = true;
  private probe: LegsInput['probeGround'];
  private probeDist = 0;
  private legReachW = 0;

  constructor(
    skeleton: CharacterSkeleton,
    private readonly pivot: THREE.Object3D,
    private readonly conf: ProceduralLegsConf
  ) {
    skeleton.root.updateWorldMatrix(true, true);
    this.rootBone = skeleton.root;
    this.rootRestY = skeleton.root.position.y;
    this.floorRestY = skeleton.floorY;
    if (skeleton.legs.length === 0) {
      throw new Error('skeleton has no legs');
    }

    const offsets = conf.offsets ?? skeleton.legs.map((_, i) => i / skeleton.legs.length);
    const hips = skeleton.legs.map(l => restPosition(skeleton, l.thigh));
    const center = hips.reduce((acc, h) => acc.add(h), new THREE.Vector3()).divideScalar(hips.length);
    skeleton.legs.forEach(({ thigh, shin, foot }, i) => {
      const knee = restPosition(skeleton, shin);
      const tip = restPosition(skeleton, foot);
      this.legs.push({
        thigh,
        shin,
        hipRest: hips[i].clone(),
        thighLen: hips[i].distanceTo(knee),
        shinLen: knee.distanceTo(tip),
        outward: _v.subVectors(hips[i], center).setY(0).normalize().clone(),
        thighRestDir: knee.clone().sub(hips[i]).normalize(),
        shinRestDir: tip.clone().sub(knee).normalize(),
        thighRestQ: thigh.getWorldQuaternion(new THREE.Quaternion()),
        shinRestQ: shin.getWorldQuaternion(new THREE.Quaternion()),
        coxaRestQInv: (thigh.parent as THREE.Object3D).getWorldQuaternion(new THREE.Quaternion()).invert(),
        offset: offsets[i],
        target: new THREE.Vector3(),
        liftoff: new THREE.Vector3(),
        inSwing: false,
      });
    });

    this.legLen = Math.min(...this.legs.map(l => l.thighLen + l.shinLen));
    if (conf.stride / 2 >= conf.reach * this.legLen) {
      throw new Error(`stride ${conf.stride} exceeds reach ${(2 * conf.reach * this.legLen).toFixed(2)}`);
    }
    const hipHeight = Math.sqrt((conf.reach * this.legLen) ** 2 - (conf.stride / 2) ** 2);
    const restHipHeight = Math.min(...this.legs.map(l => l.hipRest.y)) - this.floorRestY;
    this.walkCrouch = Math.max(0, restHipHeight - hipHeight);
  }

  get legCount() {
    return this.legs.length;
  }

  /** Forget planted feet (teleport, respawn); they re-plant under the hips on the next update. */
  reset() {
    this.needsReset = true;
  }

  update(inp: LegsInput) {
    const { conf } = this;
    const { dt } = inp;
    this.pivot.updateWorldMatrix(true, false);
    const pivotWorld = this.pivot.matrixWorld;
    _pivotInv.copy(pivotWorld).invert();
    const scale = _v.setFromMatrixScale(pivotWorld).x;
    const walking = inp.onGround && inp.walkSpeed > conf.minWalkSpeed;
    const landed = inp.landed || (inp.onGround && !this.wasOnGround);
    this.wasOnGround = inp.onGround;
    this.probe = inp.probeGround;
    this.legReachW = this.legLen * scale;
    this.probeDist = this.legReachW * ProbeMargin;

    if (inp.jumped) {
      this.airLift = conf.tuck;
    }
    this.airLift += (conf.airBend - this.airLift) * expDecay(dt, conf.jumpReleaseTau);

    // landing dip: fast attack, then recovery on its own clock (independent of the slow crouch ease-out)
    if (landed) {
      this.landTarget = conf.landCrouch;
    }
    this.landTarget *= 1 - expDecay(dt, conf.landRecoverTau);
    this.landDip +=
      (this.landTarget - this.landDip) *
      expDecay(dt, this.landTarget > this.landDip ? conf.landAttackTau : conf.landRecoverTau);

    const targetCrouch = !inp.onGround ? 0 : walking ? this.walkCrouch : conf.idleCrouch;
    const crouchTau = targetCrouch > this.crouch ? conf.crouchInTau : conf.crouchOutTau;
    this.crouch += (targetCrouch - this.crouch) * expDecay(dt, crouchTau);
    this.totalCrouch = this.crouch + this.landDip;
    this.rootBone.position.y = this.rootRestY - this.totalCrouch;

    const horizVel = _horizVel.copy(inp.bodyVel).setY(0);
    const speed = Math.max(horizVel.length(), conf.wallCadence * inp.walkSpeed);
    const strideW = conf.stride * scale;
    const cycle = walking
      ? THREE.MathUtils.clamp(
          strideW / (conf.duty * Math.max(speed, 1e-3)),
          conf.minCycleSeconds,
          conf.maxCycleSeconds
        )
      : Infinity;
    const strideEff = walking ? Math.min(strideW, speed * conf.duty * cycle) : 0;
    const moveDir = horizVel.lengthSq() > 1e-6 ? _moveDir.copy(horizVel).normalize() : _moveDir.set(0, 0, 0);
    if (walking) {
      this.phase = (this.phase + dt / cycle) % 1;
    }

    for (const leg of this.legs) {
      const hipWorldY = _v
        .copy(leg.hipRest)
        .setY(leg.hipRest.y - this.totalCrouch)
        .applyMatrix4(pivotWorld).y;
      const underHip = this.groundAt(
        _underHip.set(leg.hipRest.x, this.floorRestY, leg.hipRest.z).applyMatrix4(pivotWorld),
        hipWorldY
      );
      const ph = (this.phase + leg.offset) % 1;
      if (this.needsReset || landed) {
        // seed each foot where the cycle would have it, so no leg starts a stride behind
        const stanceProgress = walking && ph < conf.duty ? ph / conf.duty : 0.5;
        leg.target.copy(underHip).addScaledVector(moveDir, strideEff * (0.5 - stanceProgress));
        leg.inSwing = false;
      }

      if (!inp.onGround) {
        leg.target.set(
          underHip.x,
          hipWorldY - (leg.thighLen + leg.shinLen - this.airLift) * scale,
          underHip.z
        );
        leg.inSwing = false;
      } else if (!walking) {
        leg.target.lerp(underHip, expDecay(dt, conf.settleTau));
        leg.inSwing = false;
      } else if (ph < conf.duty) {
        if (leg.inSwing) {
          leg.target.copy(
            this.groundAt(_point.copy(underHip).addScaledVector(moveDir, strideEff / 2), hipWorldY)
          );
          leg.inSwing = false;
        }
      } else {
        const u = (ph - conf.duty) / (1 - conf.duty);
        if (!leg.inSwing) {
          leg.liftoff.copy(leg.target);
          leg.inSwing = true;
        }
        const remaining = (1 - u) * (1 - conf.duty) * cycle;
        const landing = this.groundAt(
          _point
            .copy(underHip)
            .addScaledVector(moveDir, strideEff / 2)
            .addScaledVector(horizVel, remaining),
          hipWorldY
        );
        leg.target.lerpVectors(leg.liftoff, landing, u);
        leg.target.y += conf.lift * scale * Math.sin(Math.PI * u);
      }
    }
    this.needsReset = false;

    for (const leg of this.legs) {
      this.solveLeg(leg);
    }
  }

  /** Ground under a world point; with nothing in reach the foot dangles at full extension. */
  private groundAt(p: THREE.Vector3, hipY: number) {
    if (this.probe) {
      p.y = this.probe(p.x, hipY, p.z, this.probeDist) ?? hipY - this.legReachW;
    }
    return p;
  }

  private solveLeg(leg: Leg) {
    const hip = _hip.copy(leg.hipRest).setY(leg.hipRest.y - this.totalCrouch);
    const foot = _foot.copy(leg.target).applyMatrix4(_pivotInv);
    const a = leg.thighLen;
    const b = leg.shinLen;
    const toFoot = _toFoot.subVectors(foot, hip);
    const d = THREE.MathUtils.clamp(toFoot.length(), Math.abs(a - b) + 1e-3, (a + b) * 0.999);
    toFoot.normalize();
    const cosHip = (a * a + d * d - b * b) / (2 * a * d);
    const sinHip = Math.sqrt(Math.max(0, 1 - cosHip * cosHip));
    // knee plane: spanned by hip→foot and this leg's outward direction
    const side = _side.copy(leg.outward).addScaledVector(toFoot, -leg.outward.dot(toFoot));
    if (side.lengthSq() < 1e-6) {
      side.set(-toFoot.y, toFoot.x, 0);
    }
    side.normalize();
    const thighDir = _thighDir.copy(toFoot).multiplyScalar(cosHip).addScaledVector(side, sinHip);
    const knee = _knee.copy(hip).addScaledVector(thighDir, a);
    const shinDir = _shinDir.subVectors(foot, knee).normalize();

    const thighQ = _q.setFromUnitVectors(leg.thighRestDir, thighDir).multiply(leg.thighRestQ);
    leg.thigh.quaternion.copy(leg.coxaRestQInv).multiply(thighQ);
    const shinQ = _q2.setFromUnitVectors(leg.shinRestDir, shinDir).multiply(leg.shinRestQ);
    leg.shin.quaternion.copy(thighQ).invert().multiply(shinQ);
  }
}
