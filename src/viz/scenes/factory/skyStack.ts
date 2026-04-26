import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { SkyStack, HorizonMode, voxelGroundLayer, gradientBackground } from 'src/viz/SkyStack';

/**
 * Builds the SkyStack used by the live factory level. Shared with
 * `factory_shader_demo` so parameter changes stay in sync between the
 * gameplay scene and the standalone shader showcase.
 */
export const buildFactorySkyStack = (viz: Viz, vizConf: VizConfig): SkyStack => {
  const skyStack = new SkyStack(
    viz,
    {
      horizonOffset: -0.038,
      horizonBlend: 0.03,
      layers: [
        voxelGroundLayer({
          id: 'voxGround',
          zIndex: 5,
          maxSteps: {
            [GraphicsQuality.Low]: 64,
            [GraphicsQuality.Medium]: 108,
            [GraphicsQuality.High]: 128,
          }[vizConf.graphics.quality],
          lavaQuality: vizConf.graphics.quality >= GraphicsQuality.High ? 1 : 0,
          oversample: vizConf.graphics.quality > GraphicsQuality.Medium ? 3 : false,
        }),
      ],
      background: gradientBackground({
        stops: [
          { position: 0.0, color: 0x411010 },
          { position: 0.3, color: 0x0 },
          { position: 1.0, color: 0x0 },
        ],
        horizonMode: HorizonMode.SolidBelow,
        belowColor: 0x060301,
        lutResolution: {
          [GraphicsQuality.Low]: 32,
          [GraphicsQuality.Medium]: 64,
          [GraphicsQuality.High]: 128,
        }[vizConf.graphics.quality],
      }),
    },
    viz.renderer.domElement.width,
    viz.renderer.domElement.height
  );
  viz.registerBeforeRenderCb(curTimeSeconds => skyStack.setTime(curTimeSeconds));
  return skyStack;
};
