import * as THREE from 'three';

import { clamp, mix } from './util/util';
import {
  DefaultThirdPersonMinPolar,
  DefaultThirdPersonMaxPolar,
  DefaultThirdPersonInitialPolar,
  DefaultThirdPersonInitialAzimuth,
  DefaultThirdPersonDistance,
  DefaultThirdPersonFOV,
  DefaultThirdPersonCameraCollisionBias,
  DefaultThirdPersonMinCameraDistance,
  DefaultThirdPersonCameraExtendSpeed,
  SoftOcclusionWidthThreshold,
  SoftOcclusionInsideMarginBase,
  SoftOcclusionInsideMarginThicknessScale,
} from './sceneDefaults';
import type { ViewMode } from './scenes/index';

const DefaultFirstPersonMinPolar = 0.1;
const DefaultFirstPersonMaxPolar = Math.PI - 0.0001;

export const DefaultZoomSpeed = 7.0;
export const DefaultFovTransitionDistance = 3.0;

export type CameraRayTestFn = (
  fromX: number,
  fromY: number,
  fromZ: number,
  toX: number,
  toY: number,
  toZ: number
) => number;

export interface CameraControllerParams {
  camera: THREE.PerspectiveCamera;
  getMouseSensitivity: () => number;
  getCameraControlEnabled: () => boolean;
  getPointerLocked: () => boolean;
  /** Returns the user-configured first-person FOV (from graphics settings). */
  getFirstPersonFOV: () => number;
  getThirdPersonXrayEnabled: () => boolean;
  cameraRayTest: CameraRayTestFn;
  /**
   * Returns the world-space hit normal from the most recent cameraRayTest call.
   * (0,0,0) when the last cast had no hit.
   */
  getLastRayHitNormal: () => { x: number; y: number; z: number };
  /** True when the most recent cameraRayTest hit a body tagged as non-permeable. */
  getLastRayHitNonPermeable: () => boolean;
}

/**
 * Unified camera controller for first-person and third-person view modes.
 *
 * Both modes share the same (phi, theta) orientation angles:
 *   - phi:   polar angle from +Y axis  (PI/2 = horizontal)
 *   - theta: azimuth angle around Y axis
 *
 * The two modes differ only in the camera distance from the player's eye.
 * At distance ≈ 0 the controller places the camera at the eye and sets rotation
 * directly from the angles (first-person). At distance > 0 it offsets the camera
 * via spherical coordinates and uses lookAt (third-person). Scroll-wheel zoom
 * moves smoothly between the two when enabled.
 */
export class CameraController {
  /** Polar angle measured from +Y. PI/2 = horizontal. */
  private phi = DefaultThirdPersonInitialPolar;
  /** Azimuth angle around Y. */
  private theta = DefaultThirdPersonInitialAzimuth;

  /** User-controlled target distance (changed by scroll wheel or configure()). */
  private targetDistance = 0;
  /** Current distance after collision clamping and smoothing. */
  private currentDistance = 0;

  private active = false;
  private minPolarAngle = DefaultFirstPersonMinPolar;
  private maxPolarAngle = DefaultFirstPersonMaxPolar;
  private maxZoomDistance = DefaultThirdPersonDistance;
  private minZoomDistance = 0;
  private zoomEnabled = false;
  private zoomSpeed = DefaultZoomSpeed;
  private thirdPersonFOV = DefaultThirdPersonFOV;
  private fovTransitionDistance = DefaultFovTransitionDistance;
  private cameraCollisionBias = DefaultThirdPersonCameraCollisionBias;
  private minCameraDistance = DefaultThirdPersonMinCameraDistance;
  private cameraExtendSpeed = DefaultThirdPersonCameraExtendSpeed;

  private readonly camera: THREE.PerspectiveCamera;
  private readonly getMouseSensitivity: () => number;
  private readonly getCameraControlEnabled: () => boolean;
  private readonly getPointerLocked: () => boolean;
  private readonly getFirstPersonFOV: () => number;
  private readonly getThirdPersonXrayEnabled: () => boolean;
  private readonly cameraRayTest: CameraRayTestFn;
  private readonly getLastRayHitNormal: () => { x: number; y: number; z: number };
  private readonly getLastRayHitNonPermeable: () => boolean;

  private readonly spherical = new THREE.Spherical();
  private readonly offset = new THREE.Vector3();
  private readonly eyePos = new THREE.Vector3();

  private _isFirstPerson = true;
  /** Set to true when the last frame used soft-occlusion dithering instead of a hard snap. */
  public isSoftOccluded = false;

  constructor(params: CameraControllerParams) {
    this.camera = params.camera;
    this.getMouseSensitivity = params.getMouseSensitivity;
    this.getCameraControlEnabled = params.getCameraControlEnabled;
    this.getPointerLocked = params.getPointerLocked;
    this.getFirstPersonFOV = params.getFirstPersonFOV;
    this.getThirdPersonXrayEnabled = params.getThirdPersonXrayEnabled;
    this.cameraRayTest = params.cameraRayTest;
    this.getLastRayHitNormal = params.getLastRayHitNormal;
    this.getLastRayHitNonPermeable = params.getLastRayHitNonPermeable;

    this.installEventListeners();
  }

  /**
   * Configure the controller for a new view mode.
   *
   * @param overrideAngles  If supplied, used instead of deriving angles from the
   *                        view-mode config or camera quaternion. Useful when the
   *                        camera's current orientation doesn't map meaningfully to
   *                        the new mode (e.g. coming from top-down).
   */
  public configure(
    viewMode: Extract<ViewMode, { type: 'firstPerson' | 'thirdPerson' }>,
    overrideAngles?: { phi: number; theta: number }
  ): void {
    this.active = true;

    if (viewMode.type === 'firstPerson') {
      this.targetDistance = 0;
      this.currentDistance = 0;
      this._isFirstPerson = true;
      this.minPolarAngle = DefaultFirstPersonMinPolar;
      this.maxPolarAngle = DefaultFirstPersonMaxPolar;
      this.zoomEnabled = false;

      if (overrideAngles) {
        this.phi = overrideAngles.phi;
        this.theta = overrideAngles.theta;
      } else {
        // Derive orientation from the current camera quaternion so that
        // switching from TP → FP preserves the look direction.
        const euler = new THREE.Euler().setFromQuaternion(this.camera.quaternion, 'YXZ');
        this.theta = euler.y;
        this.phi = euler.x + Math.PI / 2;
      }
    } else {
      const distance = viewMode.distance ?? DefaultThirdPersonDistance;

      this.minPolarAngle = viewMode.minPolarAngle ?? DefaultThirdPersonMinPolar;
      this.maxPolarAngle = viewMode.maxPolarAngle ?? DefaultThirdPersonMaxPolar;
      this.thirdPersonFOV = viewMode.cameraFOV ?? DefaultThirdPersonFOV;
      this.cameraCollisionBias = viewMode.cameraCollisionBias ?? DefaultThirdPersonCameraCollisionBias;
      this.minCameraDistance = viewMode.minCameraDistance ?? DefaultThirdPersonMinCameraDistance;
      this.cameraExtendSpeed = viewMode.cameraExtendSpeed ?? DefaultThirdPersonCameraExtendSpeed;

      this.zoomEnabled = viewMode.zoomEnabled ?? false;
      this.maxZoomDistance = viewMode.maxZoomDistance ?? distance;
      this.minZoomDistance = viewMode.minZoomDistance ?? 0;
      this.zoomSpeed = viewMode.zoomSpeed ?? DefaultZoomSpeed;
      this.fovTransitionDistance = viewMode.fovTransitionDistance ?? DefaultFovTransitionDistance;

      if (overrideAngles) {
        this.phi = overrideAngles.phi;
        this.theta = overrideAngles.theta;
      } else {
        this.phi = viewMode.initialPolarAngle ?? DefaultThirdPersonInitialPolar;
        this.theta = viewMode.initialAzimuthAngle ?? DefaultThirdPersonInitialAzimuth;
      }

      this.targetDistance = distance;
      this.currentDistance = distance;
      this._isFirstPerson = false;
    }

    this.applyFOV();
  }

  /** Mark the controller as inactive (e.g. when switching to top-down). */
  public deactivate(): void {
    this.active = false;
  }

  /** Whether the camera is effectively in first-person mode (distance ≈ 0). */
  public get isFirstPerson(): boolean {
    return this._isFirstPerson;
  }

  /** Current orientation angles. */
  public get angles(): { phi: number; theta: number } {
    return { phi: this.phi, theta: this.theta };
  }

  /** Current effective distance from the player eye after collision. */
  public get effectiveDistance(): number {
    return this.currentDistance;
  }

  /** Player-set zoom distance (before collision). 0 in first-person mode. */
  public get targetZoomDistance(): number {
    return this.targetDistance;
  }

  /**
   * Compute the ideal camera position for the current angles and target distance,
   * *without* collision.  Used to compute view-mode transition end states.
   */
  public computeIdealCameraPos(playerEyePos: THREE.Vector3): THREE.Vector3 {
    if (this.targetDistance <= 0) {
      return playerEyePos.clone();
    }
    this.spherical.set(this.targetDistance, this.phi, this.theta);
    this.offset.setFromSpherical(this.spherical);
    return playerEyePos.clone().add(this.offset);
  }

  /**
   * Compute the camera rotation for the current angles.
   *
   * In both FP and TP modes the look direction is uniquely determined by
   * (phi, theta).  The Euler mapping is:
   *   euler.x  (pitch) = phi − π/2
   *   euler.y  (yaw)   = theta
   */
  public computeLookRotation(): THREE.Euler {
    return new THREE.Euler(this.phi - Math.PI / 2, this.theta, 0, 'YXZ');
  }

  /** Current FOV based on distance and zoom state. */
  public get currentFOV(): number {
    return this.computeFOV();
  }

  /**
   * Per-frame update.  Positions and orients the camera relative to the
   * player's eye, applies collision, and manages FOV interpolation.
   */
  public update(playerEyePos: THREE.Vector3, dtSecs: number): void {
    this.eyePos.copy(playerEyePos);

    const effectiveDistance = this.targetDistance > 0 ? this.applyCollision(dtSecs) : 0;

    this._isFirstPerson = effectiveDistance < 0.01;

    if (this._isFirstPerson) {
      // First-person: camera at eye, rotation directly from angles.
      this.camera.position.copy(this.eyePos);
      this.camera.rotation.set(this.phi - Math.PI / 2, this.theta, 0, 'YXZ');
    } else {
      // Third-person: camera offset by spherical coords, looking at the eye.
      this.spherical.set(effectiveDistance, this.phi, this.theta);
      this.offset.setFromSpherical(this.spherical);
      this.camera.position.copy(this.eyePos).add(this.offset);
      this.camera.lookAt(this.eyePos);
    }

    this.applyFOV();
  }

  public setAngles(phi: number, theta: number): void {
    this.phi = phi;
    this.theta = theta;
  }

  /** Set the target zoom distance (no-op in first-person mode). */
  public setTargetZoomDistance(distance: number): void {
    if (this.targetDistance <= 0) return;
    this.targetDistance = distance;
    this.currentDistance = Math.min(this.currentDistance, distance);
  }

  public destroy(): void {
    document.body.removeEventListener('mousemove', this.handleMouseMove);
    document.body.removeEventListener('wheel', this.handleWheel);
  }

  private applyCollision(dtSecs: number): number {
    const maxDist = this.targetDistance;

    // When zoomed in past the collision floor, skip collision entirely.
    if (maxDist <= this.minCameraDistance) {
      this.isSoftOccluded = false;
      this.currentDistance = maxDist;
      return this.currentDistance;
    }

    this.spherical.set(maxDist, this.phi, this.theta);
    this.offset.setFromSpherical(this.spherical);
    const idealX = this.eyePos.x + this.offset.x;
    const idealY = this.eyePos.y + this.offset.y;
    const idealZ = this.eyePos.z + this.offset.z;

    if (this.getThirdPersonXrayEnabled()) {
      return this.applyCollisionSoft(maxDist, idealX, idealY, idealZ, dtSecs);
    }

    // SoftOcclusion disabled — plain hard snap.
    this.isSoftOccluded = false;
    const hitFraction = this.cameraRayTest(
      this.eyePos.x,
      this.eyePos.y,
      this.eyePos.z,
      idealX,
      idealY,
      idealZ
    );
    const collisionDist =
      hitFraction < 1.0
        ? Math.max(this.minCameraDistance, hitFraction * maxDist - this.cameraCollisionBias)
        : maxDist;
    if (collisionDist < this.currentDistance) {
      this.currentDistance = collisionDist;
    } else {
      this.currentDistance = Math.min(
        collisionDist,
        maxDist,
        this.currentDistance + this.cameraExtendSpeed * dtSecs
      );
    }
    return this.currentDistance;
  }

  /**
   * Collision handling with soft occlusion.  Walks forward through geometry layers one at a
   * time, measuring each layer's actual thickness (excluding gaps between separate pieces).
   * Thin layers (< SoftOcclusionWidthThreshold) are left for the shader to dither away.
   * The first layer that exceeds the threshold triggers a hard snap to just before it;
   * any thin layers between the eye and that snap point are still dithered.
   *
   * Each layer costs 2 raycasts (entry + exit).  MAX_LAYERS caps the total work.
   */
  private applyCollisionSoft(
    maxDist: number,
    idealX: number,
    idealY: number,
    idealZ: number,
    dtSecs: number
  ): number {
    // Step-inside size: small enough not to miss thin geometry, large enough to be past the hit surface.
    const STEP = 0.08; // metres
    const MAX_LAYERS = 4;

    // Unit direction from eye to ideal camera (offset has magnitude = maxDist).
    const dirX = this.offset.x / maxDist;
    const dirY = this.offset.y / maxDist;
    const dirZ = this.offset.z / maxDist;

    let probeDist = 0; // distance along ray we're currently testing from
    let hasSoftLayer = false;
    // Track the last measured layer thickness for margin scaling.
    let lastThickness = 0;

    for (let i = 0; i < MAX_LAYERS; i++) {
      const fromX = this.eyePos.x + probeDist * dirX;
      const fromY = this.eyePos.y + probeDist * dirY;
      const fromZ = this.eyePos.z + probeDist * dirZ;

      const fwdFrac = this.cameraRayTest(fromX, fromY, fromZ, idealX, idealY, idealZ);
      if (fwdFrac >= 1.0) {
        // No more geometry between here and the ideal camera.
        break;
      }

      // Entry of this layer in absolute distance from eye.
      const remaining = maxDist - probeDist;
      const entryDist = probeDist + fwdFrac * remaining;

      // Non-permeable barrier — always hard-snap regardless of thickness.
      if (this.getLastRayHitNonPermeable()) {
        this.isSoftOccluded = hasSoftLayer;
        const margin =
          SoftOcclusionInsideMarginBase + SoftOcclusionInsideMarginThicknessScale * lastThickness;
        const snapDist = Math.max(this.minCameraDistance, entryDist - this.cameraCollisionBias - margin);
        if (snapDist < this.currentDistance) {
          this.currentDistance = snapDist;
        } else {
          this.currentDistance = Math.min(snapDist, this.currentDistance + this.cameraExtendSpeed * dtSecs);
        }
        return this.currentDistance;
      }

      const insideDist = entryDist + STEP;

      if (insideDist >= maxDist) {
        // Layer entry is within STEP of the ideal camera — camera is at or inside the surface.
        // Hard-snap to just before the entry; any soft layers before this point are still dithered.
        this.isSoftOccluded = hasSoftLayer;
        const margin =
          SoftOcclusionInsideMarginBase + SoftOcclusionInsideMarginThicknessScale * lastThickness;
        const snapDist = Math.max(this.minCameraDistance, entryDist - this.cameraCollisionBias - margin);
        if (snapDist < this.currentDistance) {
          this.currentDistance = snapDist;
        } else {
          this.currentDistance = Math.min(snapDist, this.currentDistance + this.cameraExtendSpeed * dtSecs);
        }
        return this.currentDistance;
      }

      // Cast from just inside the layer to find where it exits.
      const insideX = this.eyePos.x + insideDist * dirX;
      const insideY = this.eyePos.y + insideDist * dirY;
      const insideZ = this.eyePos.z + insideDist * dirZ;
      const exitFrac = this.cameraRayTest(insideX, insideY, insideZ, idealX, idealY, idealZ);

      let exitDist: number;
      if (exitFrac >= 1.0) {
        // Forward probe found no exit — try a reverse raycast from the ideal camera inward.
        const revFrac = this.cameraRayTest(idealX, idealY, idealZ, fromX, fromY, fromZ);
        if (revFrac < 1.0) {
          // Hit found: convert fraction-from-ideal to distance-from-eye.
          const revExitDist = probeDist + (1.0 - revFrac) * remaining;
          exitDist = revExitDist > entryDist ? revExitDist : insideDist;
        } else {
          // Neither direction found an exit — treat as thin (STEP thickness) to avoid false snap.
          exitDist = insideDist;
        }
      } else {
        exitDist = insideDist + exitFrac * (maxDist - insideDist);
      }
      const thickness = exitDist - entryDist;
      lastThickness = thickness;

      if (thickness >= SoftOcclusionWidthThreshold) {
        // Too thick to dither — hard snap to just before this layer.
        // Any thin layers already accumulated between eye and here will still be dithered.
        this.isSoftOccluded = hasSoftLayer;
        const margin = SoftOcclusionInsideMarginBase + SoftOcclusionInsideMarginThicknessScale * thickness;
        const snapDist = Math.max(this.minCameraDistance, entryDist - this.cameraCollisionBias - margin);
        if (snapDist < this.currentDistance) {
          this.currentDistance = snapDist;
        } else {
          this.currentDistance = Math.min(snapDist, this.currentDistance + this.cameraExtendSpeed * dtSecs);
        }
        return this.currentDistance;
      }

      // Thin layer — mark it and continue probing from just past its exit.
      hasSoftLayer = true;
      probeDist = exitDist + STEP;
      if (probeDist >= maxDist) break;
    }

    // All layers (if any) were thin — but verify the camera itself isn't inside geometry.
    // Cast from the ideal camera position toward the eye: a back-face hit (hit normal pointing
    // in the same general direction as the ray = dot > 0) means we started inside geometry.
    const checkFrac = this.cameraRayTest(idealX, idealY, idealZ, this.eyePos.x, this.eyePos.y, this.eyePos.z);
    if (checkFrac < 1.0) {
      const n = this.getLastRayHitNormal();
      // Ray direction from idealXYZ toward eye (unnormalised — sign is all we need).
      const rdx = this.eyePos.x - idealX;
      const rdy = this.eyePos.y - idealY;
      const rdz = this.eyePos.z - idealZ;
      const dot = rdx * n.x + rdy * n.y + rdz * n.z;
      if (dot > 0) {
        // Back-face hit: camera is inside geometry — snap to just before the surface.
        const margin =
          SoftOcclusionInsideMarginBase + SoftOcclusionInsideMarginThicknessScale * lastThickness;
        const snapDist = Math.max(
          this.minCameraDistance,
          (1.0 - checkFrac) * maxDist - this.cameraCollisionBias - margin
        );
        this.isSoftOccluded = hasSoftLayer;
        if (snapDist < this.currentDistance) {
          this.currentDistance = snapDist;
        } else {
          this.currentDistance = Math.min(snapDist, this.currentDistance + this.cameraExtendSpeed * dtSecs);
        }
        return this.currentDistance;
      }
    }

    this.isSoftOccluded = hasSoftLayer;
    this.currentDistance = Math.min(maxDist, this.currentDistance + this.cameraExtendSpeed * dtSecs);
    return this.currentDistance;
  }

  private computeFOV(): number {
    if (this.zoomEnabled && this.fovTransitionDistance > 0) {
      const t = clamp(this.currentDistance / this.fovTransitionDistance, 0, 1);
      return mix(this.getFirstPersonFOV(), this.thirdPersonFOV, t);
    }
    return this._isFirstPerson ? this.getFirstPersonFOV() : this.thirdPersonFOV;
  }

  private applyFOV(): void {
    const targetFOV = this.computeFOV();
    if (Math.abs(this.camera.fov - targetFOV) > 0.01) {
      this.camera.fov = targetFOV;
      this.camera.updateProjectionMatrix();
    }
  }

  private handleMouseMove = (evt: MouseEvent): void => {
    if (!this.active || !this.getPointerLocked() || !this.getCameraControlEnabled()) {
      return;
    }

    const sensitivity = this.getMouseSensitivity();
    this.theta -= evt.movementX * sensitivity * 0.001;
    this.phi -= evt.movementY * sensitivity * 0.001;
    this.phi = clamp(this.phi, this.minPolarAngle, this.maxPolarAngle);
  };

  private handleWheel = (evt: WheelEvent): void => {
    if (!this.active || !this.zoomEnabled || !this.getPointerLocked() || !this.getCameraControlEnabled()) {
      return;
    }

    // evt.preventDefault();

    // +deltaY = scroll down = zoom out; -deltaY = scroll up = zoom in
    const delta = Math.sign(evt.deltaY);
    this.targetDistance = clamp(
      this.targetDistance + delta * this.zoomSpeed,
      this.minZoomDistance,
      this.maxZoomDistance
    );

    // Snap currentDistance down immediately when zooming in so the camera
    // doesn't lag behind the scroll wheel due to the extend-speed limit.
    if (this.targetDistance < this.currentDistance) {
      this.currentDistance = this.targetDistance;
    }
  };

  private installEventListeners(): void {
    document.body.addEventListener('mousemove', this.handleMouseMove);
    document.body.addEventListener('wheel', this.handleWheel, {
      // \/ setting this to false causes wheel events to randomly stop getting picked up for me, on Google Chrome on Linux at least.
      // passive: false
    });
  }
}
