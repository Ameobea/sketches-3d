export const buildHeightAlphaEarlyOut = (
  heightAlpha: { bottomFade?: [number, number]; topFade?: [number, number] } | undefined
): string => {
  if (!heightAlpha) return '';
  const { bottomFade, topFade } = heightAlpha;
  if (!bottomFade && !topFade) return '';

  const lines: string[] = [];
  if (bottomFade) {
    lines.push(
      /* glsl */ `heightAlphaFactor *= smoothstep(${bottomFade[0].toFixed(3)}, ${bottomFade[1].toFixed(3)}, vWorldPos.y);`
    );
  }
  if (topFade) {
    lines.push(
      /* glsl */ `heightAlphaFactor *= 1.0 - smoothstep(${topFade[0].toFixed(3)}, ${topFade[1].toFixed(3)}, vWorldPos.y);`
    );
  }

  return /* glsl */ `
    float heightAlphaFactor = 1.0;
    ${lines.join('\n    ')}
    if (heightAlphaFactor < 0.001) {
      outFragColor = vec4(0.0, 0.0, 0.0, 1.0);
      return;
    }
  `;
};

export const buildHeightAlphaFragment = (
  heightAlpha: { bottomFade?: [number, number]; topFade?: [number, number] } | undefined
): string => {
  if (!heightAlpha) return '';
  const { bottomFade, topFade } = heightAlpha;
  if (!bottomFade && !topFade) return '';

  return /* glsl */ `{
    outgoingLight.rgb = mix(outgoingLight.rgb, vec3(0.0), 1. - heightAlphaFactor);
  }`;
};
