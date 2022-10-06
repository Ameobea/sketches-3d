/*
 * Code adapted from N8 Programs: https://n8python.github.io/goodGodRays/
 *
 * With cleanup and minor changes
 */

import * as THREE from 'three';
import { Effect, EffectAttribute, Pass, Resizable } from 'postprocessing';

import GodraysVertexShader from './godrays.vert?raw';
import GodraysFragmentShader from './godrays.frag?raw';
import GodraysCompositorShader from './compositor.frag?raw';
import GodraysCompositorVertexShader from './compositor.vert?raw';

class GodraysMaterial extends THREE.ShaderMaterial {
  constructor() {
    const uniforms = {};
    super({
      name: 'GodraysMaterial',
      uniforms,
      fragmentShader: GodraysFragmentShader,
      vertexShader: GodraysVertexShader,
    });
  }
}

class GodraysPass extends Pass implements Resizable {
  private material: GodraysMaterial;

  constructor(props: GodraysEffectProps, params: GodraysEffectParams) {
    super('GodraysPass');

    this.material = new GodraysMaterial();
    this.material.uniforms.sceneDepth = { value: null };
    this.material.uniforms.lightPos = { value: props.lightPos };
    this.material.uniforms.cameraPos = { value: props.cameraPos };
    this.material.uniforms.resolution = { value: new THREE.Vector2(1, 1) };
    this.material.uniforms.projectionMatrixInv = { value: props.projectionMatrixInv };
    this.material.uniforms.viewMatrixInv = { value: props.viewMatrixInv };
    this.material.uniforms.depthCube = { value: props.depthCube };
    this.material.uniforms.mapSize = { value: props.mapSize };
    this.material.uniforms.cameraNear = { value: props.cameraNear };
    this.material.uniforms.cameraFar = { value: props.cameraFar };
    this.material.uniforms.density = { value: params.density };
    this.material.uniforms.maxDensity = { value: params.maxDensity };
    this.material.uniforms.distanceAttenuation = { value: params.distanceAttenuation };

    this.setParams(params);

    this.fullscreenMaterial = this.material;
  }

  setSize(width: number, height: number): void {
    this.material.uniforms.resolution.value.set(width, height);
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
    // this.scene.overrideMaterial = null;
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    console.log('setting godrays pass depth texture', depthTexture);
    this.material.uniforms.sceneDepth.value = depthTexture;
    if (depthPacking && depthPacking !== THREE.BasicDepthPacking) {
      throw new Error('Depth packing not supported');
    }
  }

  private createOrUpdateUniform = (name: string, value: any) => {
    if (this.material.uniforms[name]) {
      this.material.uniforms[name]!.value = value;
    } else {
      this.material.uniforms[name] = value;
    }
  };

  public setParams({ density, maxDensity, distanceAttenuation }: GodraysEffectParams): void {
    this.createOrUpdateUniform('density', density);
    this.createOrUpdateUniform('maxDensity', maxDensity);
    this.createOrUpdateUniform('distanceAttenuation', distanceAttenuation);
  }
}

interface GodraysCompositorMaterialProps {
  godrays: THREE.Texture;
  edgeStrength: number;
  edgeRadius: number;
  color: THREE.Color;
}

class GodraysCompositorMaterial extends THREE.ShaderMaterial implements Resizable {
  constructor({ godrays, edgeStrength, edgeRadius, color }: GodraysCompositorMaterialProps) {
    const uniforms = {
      godrays: { value: godrays },
      sceneDiffuse: { value: null },
      sceneDepth: { value: null },
      edgeStrength: { value: edgeStrength },
      edgeRadius: { value: edgeRadius },
      color: { value: color },
      resolution: { value: new THREE.Vector2(1, 1) },
    };

    super({
      name: 'GodraysCompositorMaterial',
      uniforms,
      depthWrite: false,
      depthTest: false,
      fragmentShader: GodraysCompositorShader,
      vertexShader: GodraysCompositorVertexShader,
    });

    this.setParams({ edgeStrength, edgeRadius, color });
  }

  public setParams({
    edgeStrength,
    edgeRadius,
    color,
  }: Pick<GodraysEffectParams, 'edgeStrength' | 'edgeRadius' | 'color'>): void {
    this.uniforms.edgeStrength.value = edgeStrength;
    this.uniforms.edgeRadius.value = edgeRadius;
    this.uniforms.color.value = color;
  }

  setSize(width: number, height: number): void {
    this.uniforms.resolution.value.set(width, height);
  }
}

class GodraysCompositorPass extends Pass implements Resizable {
  constructor(props: GodraysCompositorMaterialProps) {
    super('GodraysCompositorPass');
    this.fullscreenMaterial = new GodraysCompositorMaterial(props);
  }

  public setParams(params: GodraysEffectParams): void {
    (this.fullscreenMaterial as GodraysCompositorMaterial).setParams(params);
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    // (this.fullscreenMaterial as GodraysCompositorMaterial).uniforms.sceneDepth.value =
    //   inputBuffer.depthTexture;
    (this.fullscreenMaterial as GodraysCompositorMaterial).uniforms.sceneDiffuse.value = inputBuffer.texture;
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    console.log('Setting compositor depth texture');
    (this.fullscreenMaterial as GodraysCompositorMaterial).uniforms.sceneDepth.value = depthTexture;
  }

  setSize(width: number, height: number): void {
    (this.fullscreenMaterial as GodraysCompositorMaterial).setSize(width, height);
  }
}

interface GodraysEffectProps {
  lightPos: THREE.Vector3;
  mapSize: number;
  depthCube: THREE.CubeTexture;
  viewMatrixInv: THREE.Matrix4;
  projectionMatrixInv: THREE.Matrix4;
  cameraPos: THREE.Vector3;
  cameraNear: number;
  cameraFar: number;
}

interface GodraysEffectParams {
  density: number;
  maxDensity: number;
  edgeStrength: number;
  edgeRadius: number;
  distanceAttenuation: number;
  color: THREE.Color;
}

export class GodraysEffect extends Pass {
  private godraysRenderTarget = new THREE.WebGLRenderTarget(1, 1, {
    minFilter: THREE.LinearFilter,
    magFilter: THREE.LinearFilter,
    format: THREE.RGBAFormat,
  });
  private godraysPass: GodraysPass;

  private compositorPass: GodraysCompositorPass;

  constructor(props: GodraysEffectProps, params: GodraysEffectParams) {
    super('GodraysEffect');

    this.godraysPass = new GodraysPass(props, params);
    this.godraysPass.needsDepthTexture = true;

    this.compositorPass = new GodraysCompositorPass({
      godrays: this.godraysRenderTarget.texture,
      edgeStrength: params.edgeStrength,
      edgeRadius: params.edgeRadius,
      color: params.color,
    });
    this.compositorPass.needsDepthTexture = true;

    this.godraysPass.setParams(params);
  }

  public setParams(params: GodraysEffectParams): void {
    this.godraysPass.setParams(params);
    this.compositorPass.setParams(params);
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    this.godraysPass.render(renderer, inputBuffer, this.godraysRenderTarget);

    this.compositorPass.render(renderer, inputBuffer, this.renderToScreen ? null : outputBuffer);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    console.log('Setting godrays depth texture');
    this.godraysPass.setDepthTexture(depthTexture, depthPacking);
    this.compositorPass.setDepthTexture(depthTexture, depthPacking);
  }

  setSize(width: number, height: number): void {
    this.godraysRenderTarget.setSize(width, height);
    this.godraysPass.setSize(width, height);
    this.compositorPass.setSize(width, height);
  }
}
