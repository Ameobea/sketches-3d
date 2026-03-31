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
export const DefaultThirdPersonMinPolar = 0.15;
export const DefaultThirdPersonMaxPolar = Math.PI - 0.15;
export const DefaultThirdPersonInitialPolar = Math.PI / 3;
export const DefaultThirdPersonInitialAzimuth = Math.PI;
export const DefaultThirdPersonFOV = 75;
export const DefaultThirdPersonCameraCollisionBias = 0.25;
export const DefaultThirdPersonMinCameraDistance = 1.0;
export const DefaultThirdPersonCameraExtendSpeed = 220.0;

export const buildDefaultSceneConfig = () => ({
  viewMode: { type: 'firstPerson' as const },
});
