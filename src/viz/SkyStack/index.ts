export { SkyStack, type SkyStackParams } from './SkyStack';
export { SkyStackPass } from './SkyStackPass';
export type { Layer, BackgroundLayer, SharedModule, DefineContribution } from './types';

// Layer factories
export { starsLayer, type StarsLayerConfig } from './layers/stars';
export { cloudsLayer, type CloudsLayerConfig } from './layers/clouds';
export { buildingsLayer, type BuildingsLayerConfig } from './layers/buildings';
export { groundLayer, type GroundLayerConfig } from './layers/ground';
export { customLayer, type CustomLayerConfig } from './layers/custom';

// Background factories
export {
  gradientBackground,
  HorizonMode,
  type GradientBackgroundConfig,
  type GradientStop,
  type CloudBand,
} from './backgrounds/gradient';
export { solidBackground, type SolidBackgroundConfig } from './backgrounds/solid';
export { customBackground, type CustomBackgroundConfig } from './backgrounds/custom';
