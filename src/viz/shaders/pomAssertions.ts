import type * as THREE from 'three';

import type { CustomShaderOptions } from './customShader.types';
import type { TriplanarMappingParams } from './triplanarMapping';
import type { PomTexturing } from './pom';

export const runPOMAssertions = ({
  pom,
  triplanarUsesWorldSpace,
  generatedUVsUseWorldSpace,
  normalMap,
  clearcoatNormalMap,
  usePackedDiffuseNormalGBA,
  normalShader,
  useTriplanarMapping,
  useGeneratedUVs,
  commonShader,
  pomTexturing,
  pomHeightShader,
  pomHeightMap,
  antialiasColorShader,
  antialiasRoughnessShader,
}: {
  pom: CustomShaderOptions['pom'];
  triplanarUsesWorldSpace: boolean | undefined;
  generatedUVsUseWorldSpace: boolean | undefined;
  normalMap: THREE.Texture | undefined;
  clearcoatNormalMap: THREE.Texture | undefined;
  usePackedDiffuseNormalGBA: boolean | { lut: Uint8Array<ArrayBuffer> } | undefined;
  normalShader: string | undefined;
  useTriplanarMapping: boolean | Partial<TriplanarMappingParams> | undefined;
  useGeneratedUVs: boolean | undefined;
  commonShader: string | undefined;
  pomTexturing: PomTexturing;
  pomHeightShader: string | undefined;
  pomHeightMap: THREE.Texture | undefined;
  antialiasColorShader: boolean | undefined;
  antialiasRoughnessShader: boolean | undefined;
}) => {
  if (pom) {
    if (!pomHeightShader && !pomHeightMap) {
      throw new Error(
        '`pom` requires at least one of `shaders.pomHeightShader` (procedural) or `props.pomHeightMap` (heightmap texture)'
      );
    }
    if (pomTexturing === 'triplanar' && !triplanarUsesWorldSpace) {
      throw new Error(
        '`pom` requires world-space triplanar (`useWorldSpaceUVs` must not be false); the POM hit position is world-space'
      );
    }
    if (pomTexturing === 'generated' && !generatedUVsUseWorldSpace) {
      throw new Error(
        '`pom` with `useGeneratedUVs` requires world-space UVs (`useWorldSpaceUVs: true`); the POM hit position is world-space'
      );
    }
    if (pomTexturing === 'baseline' && normalMap) {
      throw new Error(
        '`pom` with baseline/warped UVs cannot use a normal map (no analytic tangent frame for the displaced hit); use `useTriplanarMapping` or `useGeneratedUVs`, or drop the normal map'
      );
    }
    if (pomTexturing === 'baseline' && pomHeightMap) {
      throw new Error(
        '`pom` with `pomHeightMap` requires `useTriplanarMapping` or `useGeneratedUVs` (no UV scheme at the displaced sample point under baseline)'
      );
    }
    if (pomTexturing !== 'triplanar' && (clearcoatNormalMap || usePackedDiffuseNormalGBA)) {
      throw new Error(
        '`pom` without `useTriplanarMapping` cannot use a clearcoat normal map / packed diffuse-normal map'
      );
    }
    if (normalShader) {
      throw new Error('`pom` cannot be combined with `normalShader`; both fully define `normal`');
    }
    if (pom.tangentSpace) {
      if (useTriplanarMapping || useGeneratedUVs) {
        throw new Error(
          '`pom.tangentSpace` marches in the mesh tangent frame and requires the mesh UVs (no `useTriplanarMapping` / `useGeneratedUVs`)'
        );
      }
      if (normalMap) {
        throw new Error(
          '`pom.tangentSpace` does not yet support a normal map (tangent-space normal mapping under tangent POM is future work)'
        );
      }
    }
    const pomTier = pom.tier ?? 'field';
    if (pomTier === 'projectedField' || pomTier === 'grid') {
      if (!pomHeightShader) {
        throw new Error(
          `\`pom.tier: "${pomTier}"\` requires \`shaders.pomHeightShader\` defining \`gridHeight\``
        );
      }
      if (pomHeightMap) {
        throw new Error(`\`pom.tier: "${pomTier}"\` is procedural-only; drop \`props.pomHeightMap\``);
      }
      if (pomTexturing !== 'baseline') {
        throw new Error(
          `\`pom.tier: "${pomTier}"\` owns its own world-grid projection; remove \`useTriplanarMapping\` / \`useGeneratedUVs\` / \`pom.tangentSpace\``
        );
      }
    }
    if (pomTier === 'grid') {
      if (!commonShader) {
        throw new Error(
          '`pom.tier: "grid"` requires `shaders.commonShader` declaring `struct <cellType> {…}` and `<cellType> gridComputeCell(vec2 cellId)`'
        );
      }
      if (typeof pom.cellPitch !== 'number') {
        throw new Error('`pom.tier: "grid"` requires `pom.cellPitch` (square-lattice pitch, world units)');
      }
      if (!pom.cellType) {
        throw new Error('`pom.tier: "grid"` requires `pom.cellType` (the per-cell struct type name)');
      }
    }
    if (pom.hitType) {
      if (pomTier !== 'projectedField') {
        throw new Error('`pom.hitType` is currently supported only on `pom.tier: "projectedField"`');
      }
      if (antialiasColorShader || antialiasRoughnessShader) {
        throw new Error(
          '`pom.hitType` evaluates the cell field once at the hit and shares it across slots; it is incompatible with `antialiasColorShader`/`antialiasRoughnessShader` (which oversample at multiple positions)'
        );
      }
    }
    if (pom.intersect === 'safeStep') {
      if (pomTier !== 'projectedField' && pomTier !== 'grid') {
        throw new Error('`pom.intersect: "safeStep"` requires `pom.tier: "projectedField"` or `"grid"`');
      }
      if (typeof pom.minFeatureWidth !== 'number') {
        throw new Error(
          '`pom.intersect: "safeStep"` requires `pom.minFeatureWidth` (the no-skip stride floor, projected-UV world units)'
        );
      }
    }
    if (pom.intersect === 'analytic') {
      if (pomTier !== 'projectedField') {
        throw new Error(
          '`pom.intersect: "analytic"` is currently supported only on `pom.tier: "projectedField"`'
        );
      }
      if (pom.boundedSilhouette) {
        throw new Error('`pom.intersect: "analytic"` does not support `pom.boundedSilhouette`');
      }
      if (typeof pom.minFeatureWidth !== 'number') {
        throw new Error(
          '`pom.intersect: "analytic"` requires `pom.minFeatureWidth` (used by the `safeStep` fallback)'
        );
      }
    }
  }
};
