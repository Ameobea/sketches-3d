import type * as THREE from 'three';

import type { EnvironmentConfig } from 'src/geoscript/geotoyAPIClient';
import type { Viz } from 'src/viz';
import { setSceneEnvironment } from 'src/viz/shaders/customShader';
import { generateGradientEnvironment, loadEnvironment, type SceneEnvironment } from 'src/viz/textureLoading';

// Cache the built env so geoscript re-runs don't re-run PMREM.
let cachedRenderer: THREE.WebGLRenderer | null = null;
let cachedKey: string | null = null;
let cachedEnv: SceneEnvironment | null = null;

let originalBackground: THREE.Scene['background'] | null = null;
let capturedOriginalBackground = false;

const keyFor = (env: EnvironmentConfig, url: string | undefined): string =>
  env.kind === 'gradient'
    ? `g:${env.skyColor}:${env.horizonColor}:${env.groundColor}`
    : `e:${url ?? `id:${env.textureId}`}`;

/** Builds (cached) and applies the scene env; pass `env: undefined` to clear. */
export const applyGeoscriptSceneEnvironment = async (
  viz: Viz,
  loader: THREE.ImageBitmapLoader,
  env: EnvironmentConfig | undefined,
  resolveTextureUrl: (id: number) => string | undefined
): Promise<void> => {
  if (cachedRenderer !== viz.renderer) {
    cachedRenderer = viz.renderer;
    cachedKey = null;
    cachedEnv = null;
    capturedOriginalBackground = false;
  }

  if (!capturedOriginalBackground) {
    originalBackground = viz.scene.background;
    capturedOriginalBackground = true;
  }

  if (!env) {
    cachedKey = null;
    cachedEnv = null;
    setSceneEnvironment(viz.scene, null);
    viz.scene.background = originalBackground;
    return;
  }

  const url = env.kind === 'equirect' ? resolveTextureUrl(env.textureId) : undefined;
  if (env.kind === 'equirect' && !url) {
    // Texture metadata not loaded yet; the caller re-invokes when it arrives.
    return;
  }

  const key = keyFor(env, url);
  if (key !== cachedKey) {
    cachedEnv =
      env.kind === 'gradient'
        ? generateGradientEnvironment(viz.renderer, {
            skyColor: env.skyColor,
            horizonColor: env.horizonColor,
            groundColor: env.groundColor,
          })
        : await loadEnvironment(viz.renderer, loader, url!);
    cachedKey = key;
  }

  const built = cachedEnv!;
  setSceneEnvironment(viz.scene, { envMap: built.envMap, intensity: env.intensity ?? 1 });
  viz.scene.background = env.setBackground === false ? originalBackground : built.background;
};
