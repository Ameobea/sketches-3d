import * as DetectGPU from 'detect-gpu';
import { writable, type Writable } from 'svelte/store';

import { mergeDeep } from './util/util';
import { rwritable, type TransparentWritable } from './util/TransparentWritable';

export const DefaultSceneName = 'bridge';

export const DefaultPlayerColliderHeight = 4.55;
export const DefaultPlayerColliderRadius = 0.35;
export const DEFAULT_FOV = 110;

export enum GraphicsQuality {
  Low = 1,
  Medium = 2,
  High = 3,
}

export const formatGraphicsQuality = (quality: GraphicsQuality): string =>
  ({
    [GraphicsQuality.Low]: 'low',
    [GraphicsQuality.Medium]: 'medium',
    [GraphicsQuality.High]: 'high',
  })[quality];

export interface GraphicsSettings {
  quality: GraphicsQuality;
  fov: number;
}

export interface AudioSettings {
  globalVolume: number;
  musicVolume: number;
  sfxVolume: number;
}

export interface GameplaySettings {
  /**
   * If easy mode is true, then magnitude is normalized to what it would be if the user was moving
   * diagonally, allowing for easier movement.
   */
  easyModeMovement: boolean;
}

export interface ControlsSettings {
  mouseSensitivity: number;
}

/**
 * Config that is persisted between runs and shared between scenes
 */
export interface VizConfig {
  graphics: GraphicsSettings;
  audio: AudioSettings;
  gameplay: GameplaySettings;
  controls: ControlsSettings;
}

const getGraphicsQuality = async (tierRes: DetectGPU.TierResult) => {
  try {
    if (tierRes.tier === 0) {
      console.warn('Potentially unsupported GPU detected');
      return GraphicsQuality.Medium;
    }

    if (tierRes.tier === 1) {
      return GraphicsQuality.Low;
    } else if (tierRes.tier === 2) {
      return GraphicsQuality.Medium;
    }

    return GraphicsQuality.High;
  } catch (err) {
    console.error('Error getting GPU tier', err);
    return GraphicsQuality.Medium;
  }
};

const getGPUPerformanceInfo = async (): Promise<{ graphicsQuality: GraphicsQuality; gpuName: string }> => {
  if (localStorage.getItem('gpuInfo')) {
    const gpuInfo = JSON.parse(localStorage.getItem('gpuInfo') ?? '{}') as {
      graphicsQuality: GraphicsQuality;
      gpuName: string;
    };
    return gpuInfo;
  }

  const tierRes = await DetectGPU.getGPUTier();
  const graphicsQuality = await getGraphicsQuality(tierRes);
  const gpuName = tierRes.gpu ?? 'unknown';

  localStorage.setItem('gpuInfo', JSON.stringify({ graphicsQuality, gpuName }));

  return { graphicsQuality, gpuName };
};

const buildDefaultVizConfig = (): VizConfig => ({
  graphics: { quality: GraphicsQuality.High, fov: DEFAULT_FOV },
  audio: { globalVolume: 0.4, musicVolume: 0.4, sfxVolume: 0.4 },
  gameplay: { easyModeMovement: true },
  controls: { mouseSensitivity: 2 },
});

/**
 * Creates an appropriate initial viz config for a user based on estimated system performance
 */
const buildInitialVizConfig = async (): Promise<VizConfig> => {
  const { graphicsQuality, gpuName } = await getGPUPerformanceInfo();
  console.log(
    `Determined initial graphics quality of "${formatGraphicsQuality(
      graphicsQuality
    )}" for detected GPU ${gpuName}`
  );

  return { ...buildDefaultVizConfig(), graphics: { quality: graphicsQuality, fov: DEFAULT_FOV } };
};

/**
 * Loads the viz config from local storage, merging it with the default viz config
 * if it's not found or if any fields are missing.
 */
export const loadVizConfig = (): VizConfig => {
  const vizConfig = JSON.parse(globalThis.localStorage?.getItem('vizConfig') ?? '{}') as VizConfig;
  return mergeDeep(buildDefaultVizConfig(), vizConfig);
};

export const getVizConfig = async (): Promise<TransparentWritable<VizConfig>> => {
  if (localStorage.getItem('vizConfig')) {
    const vizConfig = loadVizConfig();
    return rwritable(vizConfig);
  }

  const config = await buildInitialVizConfig();
  localStorage.setItem('vizConfig', JSON.stringify(config));
  return rwritable(config);
};
