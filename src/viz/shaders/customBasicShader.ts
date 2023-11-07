import * as THREE from 'three';
import { UniformsLib } from 'three';

interface CustomBasicShaderProps {
  name?: string;
  color?: THREE.Color;
  transparent?: boolean;
  alphaTest?: number;
  fogMultiplier?: number;
}

interface CustomBasicShaderShaders {
  colorShader?: string;
  vertexShader?: string;
}

interface CustomBasicShaderOptions {
  enableFog?: boolean;
}

const buildCustomBasicShaderArgs = (
  { color = new THREE.Color(0xffffff), transparent, alphaTest, fogMultiplier }: CustomBasicShaderProps = {},
  { colorShader, vertexShader }: CustomBasicShaderShaders = {},
  { enableFog = true }: CustomBasicShaderOptions = {}
) => {
  const uniforms = THREE.UniformsUtils.merge([
    UniformsLib.common,
    // UniformsLib.envmap,
    // UniformsLib.aomap,
    // UniformsLib.lightmap,
    // UniformsLib.emissivemap,
    // UniformsLib.bumpmap,
    // UniformsLib.normalmap,
    // UniformsLib.displacementmap,
    // UniformsLib.roughnessmap,
    // UniformsLib.metalnessmap,
    UniformsLib.fog,
    UniformsLib.lights,
    {
      // emissive: { value: new THREE.Color(0x000000) },
      // roughness: { value: 1.0 },
      // metalness: { value: 0.0 },
      // envMapIntensity: { value: 1 },
    },
  ]);

  uniforms.curTimeSeconds = { type: 'f', value: 0.0 };
  uniforms.diffuse = { type: 'c', value: color };

  const buildRunColorShaderFragment = () => {
    if (!colorShader) {
      return '';
    }

    return 'diffuseColor = getFragColor(diffuseColor.xyz, pos, vNormalAbsolute, curTimeSeconds, ctx);';
  };

  return {
    fog: true,
    lights: true,
    // dithering: true,
    uniforms,
    vertexShader: `
#include <common>
#include <uv_pars_vertex>
#include <envmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>

varying vec3 vWorldPosition;

uniform float curTimeSeconds;
varying vec3 pos;
varying vec3 vNormalAbsolute;

void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphcolor_vertex>
	#if defined ( USE_ENVMAP ) || defined ( USE_SKINNING )
		#include <beginnormal_vertex>
		#include <morphnormal_vertex>
		#include <skinbase_vertex>
		#include <skinnormal_vertex>
		#include <defaultnormal_vertex>
	#endif
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <worldpos_vertex>
	#include <envmap_vertex>
	#include <fog_vertex>

  vec4 worldPositionMine = vec4( transformed, 1.0 );
  worldPositionMine = modelMatrix * worldPositionMine;
  pos = worldPositionMine.xyz;

  ${vertexShader ?? ''}
}`,
    fragmentShader: `
uniform vec3 diffuse;
uniform float opacity;
#ifndef FLAT_SHADED
  varying vec3 vNormal;
#endif
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_pars_fragment>
#include <cube_uv_reflection_fragment>
#include <fog_pars_fragment>
#include <specularmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>

uniform float curTimeSeconds;
varying vec3 pos;
varying vec3 vWorldPosition;
varying vec3 vNormalAbsolute;

struct SceneCtx {
  vec3 cameraPosition;
  vec2 vUv;
  vec4 diffuseColor;
};

${colorShader ?? ''}

void main() {
  #include <clipping_planes_fragment>
  vec4 diffuseColor = vec4( diffuse, opacity );
  #include <logdepthbuf_fragment>
  #include <map_fragment>

  #if !defined(USE_UV)
    vec2 vUv = vec2(0.);
  #endif

  SceneCtx ctx = SceneCtx(cameraPosition, vUv, diffuseColor);
  ${buildRunColorShaderFragment()}

  #include <color_fragment>
  #include <alphamap_fragment>
  #include <alphatest_fragment>
  #include <specularmap_fragment>
  ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
  // accumulation (baked indirect lighting only)
  #ifdef USE_LIGHTMAP
    vec4 lightMapTexel = texture2D( lightMap, vUv2 );
    reflectedLight.indirectDiffuse += lightMapTexel.rgb * lightMapIntensity * RECIPROCAL_PI;
  #else
    reflectedLight.indirectDiffuse += vec3( 1.0 );
  #endif
  // modulation
  #include <aomap_fragment>
  reflectedLight.indirectDiffuse *= diffuseColor.rgb;
  vec3 outgoingLight = reflectedLight.indirectDiffuse;
  #include <envmap_fragment>
  #include <opaque_fragment>
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
  ${
    enableFog
      ? `
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogFactor = 1.0 - exp( - fogDensity * fogDensity * vFogDepth * vFogDepth );
    #else
      float fogFactor = smoothstep( fogNear, fogFar, vFogDepth );
    #endif
    ${typeof fogMultiplier === 'number' ? `fogFactor *= ${fogMultiplier.toFixed(4)};` : ''}
    gl_FragColor.rgb = mix( gl_FragColor.rgb, fogColor, fogFactor );
  #endif
  `
      : ''
  }
  #include <premultiplied_alpha_fragment>
  #include <dithering_fragment>
}`,
  };
};

export class CustomBasicShaderMaterial extends THREE.ShaderMaterial {
  public setCurTimeSeconds(curTimeSeconds: number) {
    this.uniforms.curTimeSeconds.value = curTimeSeconds;
  }
}

export const buildCustomBasicShader = (
  props: CustomBasicShaderProps = {},
  shaders?: CustomBasicShaderShaders,
  opts?: CustomBasicShaderOptions
) => {
  const mat = new CustomBasicShaderMaterial(buildCustomBasicShaderArgs(props, shaders, opts));
  if (props.name) {
    mat.name = props.name;
  }
  if (props.transparent) {
    (mat as any).transparent = props.transparent;
  }
  if (typeof props.alphaTest === 'number') {
    (mat as any).alphaTest = props.alphaTest;
    (mat as any).uniforms.alphaTest.value = props.alphaTest;
  }
  mat.needsUpdate = true;
  mat.uniformsNeedUpdate = true;

  return mat;
};
