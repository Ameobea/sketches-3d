import * as THREE from 'three';
import { UniformsLib } from 'three';

import noiseShaders from './noise.frag?raw';
import commonShaderCode from './common.frag?raw';

const buildNoiseTexture = (): THREE.DataTexture => {
  const noise = new Float32Array(256 * 256 * 4);
  for (let i = 0; i < noise.length; i++) {
    noise[i] = Math.random();
  }
  const texture = new THREE.DataTexture(
    noise,
    256,
    256,
    THREE.RGBAFormat,
    THREE.FloatType,
    undefined,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    // We need linear interpolation for the noise texture
    THREE.LinearFilter,
    THREE.LinearFilter
  );
  texture.needsUpdate = true;
  return texture;
};

const AntialiasedRoughnessShaderFragment = `
  float acc = 0.;
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = fragPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getCustomRoughness(offsetPos, vNormalAbsolute, curTimeSeconds);
      }
    }
  }
  acc /= 8.;
  roughnessFactor = acc;
`;

const NonAntialiasedRoughnessShaderFragment = `roughnessFactor = roughnessFactor = getCustomRoughness(pos, vNormalAbsolute, curTimeSeconds, ctx);`;

const buildRoughnessShaderFragment = (antialiasRoughnessShader?: boolean) => {
  if (antialiasRoughnessShader) {
    return AntialiasedRoughnessShaderFragment;
  }

  return NonAntialiasedRoughnessShaderFragment;
};

interface CustomShaderProps {
  roughness?: number;
  metalness?: number;
  color?: THREE.Color;
  normalScale?: number;
}

export const buildCustomShader = (
  { roughness = 0.9, metalness = 0, color = new THREE.Color(0xcbcbcb), normalScale = 1 }: CustomShaderProps,
  {
    customVertexFragment,
    colorShader,
    normalShader,
    roughnessShader,
  }: {
    customVertexFragment?: string;
    colorShader?: string;
    normalShader?: string;
    roughnessShader?: string;
  } = {},
  {
    antialiasColorShader,
    antialiasRoughnessShader,
  }: { antialiasColorShader?: boolean; antialiasRoughnessShader?: boolean } = {}
) => {
  const uniforms = THREE.UniformsUtils.merge([
    UniformsLib.common,
    UniformsLib.envmap,
    UniformsLib.aomap,
    UniformsLib.lightmap,
    // UniformsLib.emissivemap,
    // UniformsLib.bumpmap,
    UniformsLib.normalmap,
    UniformsLib.displacementmap,
    // UniformsLib.roughnessmap,
    // UniformsLib.metalnessmap,
    UniformsLib.fog,
    UniformsLib.lights,
    {
      emissive: { value: new THREE.Color(0x000000) },
      roughness: { value: 1.0 },
      metalness: { value: 0.0 },
      envMapIntensity: { value: 1 },
    },
  ]);
  uniforms.normalScale = { type: 'v2', value: new THREE.Vector2(normalScale, normalScale) };

  uniforms.roughness = { type: 'f', value: roughness };
  uniforms.metalness = { type: 'f', value: metalness };
  uniforms.ior = { type: 'f', value: 1.5 };
  uniforms.clearcoat = { type: 'f', value: 0.0 };
  uniforms.clearcoatRoughness = { type: 'f', value: 0.0 };
  uniforms.clearcoatNormal = { type: 'f', value: 0.0 };
  uniforms.transmission = { type: 'f', value: 0.0 };

  uniforms.curTimeSeconds = { type: 'f', value: 0.0 };
  uniforms.diffuse = { type: 'c', value: color };

  const buildRunColorShaderFragment = () => {
    if (!colorShader) {
      return '';
    }

    if (antialiasColorShader) {
      return `
  vec3 acc = vec3(0.);
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = fragPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getFragColor(diffuseColor.xyz, offsetPos, vNormalAbsolute, curTimeSeconds, ctx);
      }
    }
  }
  acc /= 8.;
  diffuseColor.xyz = acc;`;
    } else {
      return `
  diffuseColor.xyz = getFragColor(diffuseColor.xyz, pos, vNormalAbsolute, curTimeSeconds, ctx);`;
    }
  };

  return {
    fog: true,
    lights: true,
    // dithering: true,
    uniforms,
    vertexShader: `#define STANDARD
    varying vec3 vViewPosition;
    // #ifdef USE_TRANSMISSION
      varying vec3 vWorldPosition;
    // #endif
    #include <common>
    #include <uv_pars_vertex>
    #include <uv2_pars_vertex>
    #include <displacementmap_pars_vertex>
    #include <color_pars_vertex>
    #include <fog_pars_vertex>
    #include <normal_pars_vertex>
    #include <morphtarget_pars_vertex>
    #include <skinning_pars_vertex>
    #include <shadowmap_pars_vertex>
    #include <logdepthbuf_pars_vertex>
    #include <clipping_planes_pars_vertex>

    ${customVertexFragment ? noiseShaders : ''}

    uniform float curTimeSeconds;
    varying vec3 pos;
    varying vec3 vNormalAbsolute;

    void main() {
      // pos = position;
      vNormalAbsolute = normal;

      #include <uv_vertex>
      #include <uv2_vertex>
      #include <color_vertex>
      #include <morphcolor_vertex>
      #include <beginnormal_vertex>
      #include <morphnormal_vertex>
      #include <skinbase_vertex>
      #include <skinnormal_vertex>
      #include <defaultnormal_vertex>
      #include <normal_vertex>
      #include <begin_vertex>
      #include <morphtarget_vertex>
      #include <skinning_vertex>
      #include <displacementmap_vertex>
      #include <project_vertex>
      #include <logdepthbuf_vertex>
      #include <clipping_planes_vertex>
      vViewPosition = - mvPosition.xyz;
      #include <worldpos_vertex>
      #include <shadowmap_vertex>
      #include <fog_vertex>
    // #ifdef USE_TRANSMISSION
      vec4 worldPosition = vec4( transformed, 1.0 );
      worldPosition = modelMatrix * worldPosition;
      vWorldPosition = worldPosition.xyz;
      pos = vWorldPosition;
    // #endif

      ${customVertexFragment ?? ''}
    }`,
    fragmentShader: `
#define STANDARD
#ifdef PHYSICAL
	#define IOR
	#define SPECULAR
#endif
uniform vec3 diffuse;
uniform vec3 emissive;
uniform float roughness;
uniform float metalness;
uniform float opacity;
#ifdef IOR
	uniform float ior;
#endif
#ifdef SPECULAR
	uniform float specularIntensity;
	uniform vec3 specularColor;
	#ifdef USE_SPECULARINTENSITYMAP
		uniform sampler2D specularIntensityMap;
	#endif
	#ifdef USE_SPECULARCOLORMAP
		uniform sampler2D specularColorMap;
	#endif
#endif
#ifdef USE_CLEARCOAT
	uniform float clearcoat;
	uniform float clearcoatRoughness;
#endif
#ifdef USE_IRIDESCENCE
	uniform float iridescence;
	uniform float iridescenceIOR;
	uniform float iridescenceThicknessMinimum;
	uniform float iridescenceThicknessMaximum;
#endif
#ifdef USE_SHEEN
	uniform vec3 sheenColor;
	uniform float sheenRoughness;
	#ifdef USE_SHEENCOLORMAP
		uniform sampler2D sheenColorMap;
	#endif
	#ifdef USE_SHEENROUGHNESSMAP
		uniform sampler2D sheenRoughnessMap;
	#endif
#endif
varying vec3 vViewPosition;
#include <common>
#include <packing>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <uv2_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <bsdfs>
#include <iridescence_fragment>
#include <cube_uv_reflection_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_physical_pars_fragment>
#include <fog_pars_fragment>
#include <lights_pars_begin>
#include <normal_pars_fragment>
#include <lights_physical_pars_fragment>
#include <transmission_pars_fragment>
#include <shadowmap_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <clearcoat_pars_fragment>
#include <iridescence_pars_fragment>
#include <roughnessmap_pars_fragment>
#include <metalnessmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>

uniform float curTimeSeconds;
varying vec3 pos;
varying vec3 vWorldPosition;
varying vec3 vNormalAbsolute;
${normalShader ? 'uniform mat3 normalMatrix;' : ''}

struct SceneCtx {
  vec3 cameraPosition;
  vec2 vUv;
};

${commonShaderCode}
${noiseShaders}
${colorShader ?? ''}
${normalShader ?? ''}
${roughnessShader ?? ''}

void main() {
	#include <clipping_planes_fragment>

  vec3 fragPos = vWorldPosition;
  vec3 cameraPos = cameraPosition;
  float distanceToCamera = distance(cameraPos, fragPos);
  float unitsPerPx = abs(2. * distanceToCamera * tan(0.001 / 2.));

  vec4 diffuseColor = vec4(diffuse, opacity);

  #if !defined(USE_UV)
    vec2 vUv = vec2(0.);
  #endif

  SceneCtx ctx = SceneCtx(cameraPosition, vUv);

	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <roughnessmap_fragment>
	#include <metalnessmap_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	#include <clearcoat_normal_fragment_begin>
	#include <clearcoat_normal_fragment_maps>

  ${buildRunColorShaderFragment()}

  ${
    normalShader
      ? `
  normal = getCustomNormal(pos, vNormalAbsolute, curTimeSeconds);
  normal = normalize(normalMatrix * normal);
  `
      : ''
  }

  ${roughnessShader ? buildRoughnessShaderFragment() : ''}

	#include <emissivemap_fragment>
	// accumulation
	#include <lights_physical_fragment>
	#include <lights_fragment_begin>
	#include <lights_fragment_maps>
	#include <lights_fragment_end>
	// modulation
	#include <aomap_fragment>
	vec3 totalDiffuse = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse;
	vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;
	#include <transmission_fragment>
	vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;
	#ifdef USE_SHEEN
		// Sheen energy compensation approximation calculation can be found at the end of
		// https://drive.google.com/file/d/1T0D1VSyR4AllqIJTQAraEIzjlb5h4FKH/view?usp=sharing
		float sheenEnergyComp = 1.0 - 0.157 * max3( material.sheenColor );
		outgoingLight = outgoingLight * sheenEnergyComp + sheenSpecular;
	#endif
	#ifdef USE_CLEARCOAT
		float dotNVcc = saturate( dot( geometry.clearcoatNormal, geometry.viewDir ) );
		vec3 Fcc = F_Schlick( material.clearcoatF0, material.clearcoatF90, dotNVcc );
		outgoingLight = outgoingLight * ( 1.0 - material.clearcoat * Fcc ) + clearcoatSpecular * material.clearcoat;
	#endif
	#include <output_fragment>
	#include <tonemapping_fragment>
	#include <encodings_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,
  };
};
