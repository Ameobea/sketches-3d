import { getGPUTier, type TierResult } from 'detect-gpu';

export const DefaultSceneName = 'bridge';

export const DefaultPlayerColliderHeight = 4.55;
export const DefaultPlayerColliderRadius = 0.35;

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

/**
 * Config that is persisted between runs and shared between scenes
 */
export interface VizConfig {
  graphics: {
    quality: GraphicsQuality;
  };
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

export const getVizConfig = async (): Promise<VizConfig> => {
  if (localStorage.getItem('vizConfig')) {
    const vizConfig = JSON.parse(localStorage.getItem('vizConfig') ?? '{}') as VizConfig;
    return vizConfig;
  }

  const { graphicsQuality, gpuName } = await getGPUPerformanceInfo();
  console.log(
    `Determined initial graphics quality of "${formatGraphicsQuality(
      graphicsQuality
    )}" for detected GPU ${gpuName}`
  );
  const config = { graphics: { quality: graphicsQuality } };
  localStorage.setItem('vizConfig', JSON.stringify(config));
  return config;
};

export const getVizConfigSync = (): VizConfig => {
  if (localStorage.getItem('vizConfig')) {
    const vizConfig = JSON.parse(localStorage.getItem('vizConfig') ?? '{}') as VizConfig;
    return vizConfig;
  }

  throw new Error('Viz config not yet initialized');
};
