import { getGPUTier, type TierResult } from 'detect-gpu';

import { mergeDeep } from './util';

export const DefaultSceneName = 'bridge';

export const DefaultPlayerColliderHeight = 4.55;
export const DefaultPlayerColliderRadius = 0.35;
export const DEFAULT_FOV = 75;

export interface PlayerMoveSpeed {
  onGround: number;
  inAir: number;
}

export const DefaultMoveSpeed: PlayerMoveSpeed = Object.freeze({
  onGround: 12,
  inAir: 12,
});

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
  }[quality]);

export interface GraphicsSettings {
  quality: GraphicsQuality;
  fov: number;
}

export interface AudioSettings {
  globalVolume: number;
  musicVolume: number;
}

/**
 * Config that is persisted between runs and shared between scenes
 */
export interface VizConfig {
  graphics: GraphicsSettings;
  audio: AudioSettings;
}

const getGraphicsQuality = async (tierRes: TierResult) => {
  try {
    const tierRes = await getGPUTier();
    if (tierRes.tier === 0) {
      console.warn('Potentially unsupported GPU detected');
      return GraphicsQuality.Low;
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

  const tierRes = await getGPUTier();
  const graphicsQuality = await getGraphicsQuality(tierRes);
  const gpuName = tierRes.gpu ?? 'unknown';

  localStorage.setItem('gpuInfo', JSON.stringify({ graphicsQuality, gpuName }));

  return { graphicsQuality, gpuName };
};

const buildDefaultVizConfig = (): VizConfig => ({
  graphics: { quality: GraphicsQuality.High, fov: DEFAULT_FOV },
  audio: { globalVolume: 0.4, musicVolume: 0.4 },
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
  const vizConfig = JSON.parse(localStorage.getItem('vizConfig') ?? '{}') as VizConfig;
  return mergeDeep(buildDefaultVizConfig(), vizConfig);
};

export const getVizConfig = async (): Promise<VizConfig> => {
  if (localStorage.getItem('vizConfig')) {
    const vizConfig = loadVizConfig();
    return vizConfig;
  }

  const config = await buildInitialVizConfig();
  localStorage.setItem('vizConfig', JSON.stringify(config));
  return config;
};
