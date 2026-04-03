import type { DashConfig, PlayerMoveSpeed } from './scenes/index';

export const DefaultMoveSpeed: PlayerMoveSpeed = Object.freeze({
  onGround: 12,
  inAir: 12,
});

export const DefaultDashConfig: DashConfig = Object.freeze({
  enable: true,
  dashMagnitude: 16,
  minDashDelaySeconds: 0.85,
});

export const DefaultOOBThreshold = -55;

export const DefaultTopDownCameraFOV = 40;
export const DefaultTopDownCameraFocusPoint = { type: 'player' as const };

export const DefaultThirdPersonDistance = 15;
export const DefaultThirdPersonMinPolar = 0.05;
export const DefaultThirdPersonMaxPolar = Math.PI - 0.05;
export const DefaultThirdPersonInitialPolar = Math.PI / 3;
export const DefaultThirdPersonInitialAzimuth = Math.PI;
export const DefaultThirdPersonFOV = 75;
export const DefaultThirdPersonCameraCollisionBias = 0.25;
export const DefaultThirdPersonMinCameraDistance = 1.0;
export const DefaultThirdPersonCameraExtendSpeed = 220.0;

export const buildDefaultSceneConfig = () => ({
  viewMode: { type: 'firstPerson' as const },
});

/** Default persisted value for the soft camera occlusion / third-person x-ray feature. */
export const SoftOcclusionEnabled = true;
/**
 * Maximum thickness (meters) of an occluding object for it to be treated as "soft".
 * If the reverse raycast shows the occluder is thicker than this, the camera snaps normally.
 */
export const SoftOcclusionWidthThreshold = 20.0;
/** Radius (meters) of the dithered cylinder revealed around the camera-to-eye segment. */
export const SoftOcclusionRevealRadius = 3.5;
/** Fade width (meters) at the outer edge of the reveal cylinder. */
export const SoftOcclusionRevealFade = 0.7;
/** World-space distance (meters) ahead of the player eye over which occlusion fades in. */
export const SoftOcclusionEyeMargin = 4.0;
/**
 * Base margin (meters) added to the inside-geometry snap distance.
 * Keeps the camera from being placed right up against an occluding surface.
 * The actual margin is: base + thicknessScale * measuredThickness.
 */
export const SoftOcclusionInsideMarginBase = 0.5;
/** Fraction of the measured occluder thickness added to the snap margin. */
export const SoftOcclusionInsideMarginThicknessScale = 0.05;
