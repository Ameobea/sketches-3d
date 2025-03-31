import type { Viz } from 'src/viz';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import BridgeMistColorShader from 'src/viz/shaders/bridge2/bridge_top_mist/color.frag?raw';

export const buildCheckpointMaterial = (viz: Viz, color: [number, number, number] = [0.8, 0.5, 0.6]) => {
  const mat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    {
      colorShader: BridgeMistColorShader.replace(
        'vec4 outColor = vec4(0.8, 0.5, 0.6, 0.0);',
        `vec4 outColor = vec4(${color[0].toFixed(8)}, ${color[1].toFixed(8)}, ${color[2].toFixed(8)}, 0.0);`
      ),
    },
    { disableToneMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => mat.setCurTimeSeconds(curTimeSeconds));
  return mat;
};
