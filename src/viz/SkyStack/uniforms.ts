import * as THREE from 'three';

/**
 * Compositor-shared uniforms for a SkyStack. Every layer's GLSL has these in
 * scope via the prelude. Per-layer uniforms (star color, building counts,
 * gradient stops, …) live on the individual `Layer` / `BackgroundLayer`
 * objects — not here.
 */
export interface SkyStackSharedUniforms {
  uTime: THREE.IUniform<number>;
  uHorizonOffset: THREE.IUniform<number>;
  /** Half-width of the horizon-smoothstep band, in elevation units. */
  uHorizonBlend: THREE.IUniform<number>;
  uProjectionMatrixInverse: THREE.IUniform<THREE.Matrix4>;
  uCameraWorldMatrix: THREE.IUniform<THREE.Matrix4>;
  /** Stable scene depth texture for occlusion discard. */
  uSceneDepth: THREE.IUniform<THREE.Texture | null>;
}

export const createSharedUniforms = (): SkyStackSharedUniforms => ({
  uTime: { value: 0 },
  uHorizonOffset: { value: 0 },
  uHorizonBlend: { value: 0.02 },
  uProjectionMatrixInverse: { value: new THREE.Matrix4() },
  uCameraWorldMatrix: { value: new THREE.Matrix4() },
  uSceneDepth: { value: null },
});

export const asUniformRecord = (u: SkyStackSharedUniforms): Record<string, THREE.IUniform> =>
  u as unknown as Record<string, THREE.IUniform>;
