import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import {
  configureDefaultPostprocessingPipeline,
  type PostprocessingPipelineController,
} from 'src/viz/postprocessing/defaultPostprocessing';

/**
 * The mesh mode's postprocessing pipeline — constructed once per load, before the app
 * mounts (quality changes reload the page). Same call and args as the game default so
 * geotoy renders match in-game output by construction.
 */
export const buildMeshPipeline = (
  viz: Viz,
  quality: GraphicsQuality,
  renderMode: boolean
): PostprocessingPipelineController =>
  configureDefaultPostprocessingPipeline({
    viz,
    quality,
    addMiddlePasses: (composer, viz, _quality) => {
      if (quality > GraphicsQuality.Low && (window.innerWidth > 800 || renderMode)) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        composer.addPass(n8aoPass);
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'Medium',
          }[quality]
        );
      }
    },
    autoUpdateShadowMap: !renderMode,
    toneMapping: { mode: 'neutral', exposure: 1 },
    emissiveBypass: true,
    // runtimeThreshold: the per-tab bloom knobs drive threshold/softKnee live via
    // setEmissiveBloom, which needs the filter chain allocated up front.
    emissiveBloom: { runtimeThreshold: true },
    pomExitBuffers: true,
  });
