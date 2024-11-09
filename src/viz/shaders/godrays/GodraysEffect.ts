/*
 * Code taken + adapted from this demo: https://n8python.github.io/goodGodRays/
 * By: https://github.com/n8python
 *
 * With cleanup and minor changes
 */

import { type Disposable, Pass, type Resizable } from 'postprocessing';
import * as THREE from 'three';

import GodraysCompositorShader from './compositor.frag?raw';
import GodraysCompositorVertexShader from './compositor.vert?raw';
import GodraysFragmentShader from './godrays.frag?raw';
import GodraysVertexShader from './godrays.vert?raw';

const GODRAYS_RESOLUTION_SCALE = 0.5;

class GodraysMaterial extends THREE.ShaderMaterial {
  constructor(blueNoiseTexture: THREE.Texture) {
    const uniforms = {
      density: { value: 1 / 128 },
      maxDensity: { value: 0.5 },
      distanceAttenuation: { value: 0.005 },
      sceneDepth: { value: null },
      lightPos: { value: new THREE.Vector3(0, 0, 0) },
      cameraPos: { value: new THREE.Vector3(0, 0, 0) },
      resolution: { value: new THREE.Vector2(1, 1) },
      projectionMatrixInv: { value: new THREE.Matrix4() },
      viewMatrixInv: { value: new THREE.Matrix4() },
      depthCube: { value: null },
      mapSize: { value: 1 },
      pointLightCameraNear: { value: 0.1 },
      pointLightCameraFar: { value: 1000 },
      blueNoise: { value: blueNoiseTexture },
      noiseResolution: {
        value: new THREE.Vector2(blueNoiseTexture.image.width, blueNoiseTexture.image.height),
      },
    };

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

    this.material = new GodraysMaterial(props.blueNoiseTexture);

    this.updateUniforms(props, params);

    this.fullscreenMaterial = this.material;
  }

  setSize(width: number, height: number): void {
    this.material.uniforms.resolution.value.set(
      Math.ceil(width * GODRAYS_RESOLUTION_SCALE),
      Math.ceil(height * GODRAYS_RESOLUTION_SCALE)
    );
  }

  render(
    renderer: THREE.WebGLRenderer,
    _inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    this.material.uniforms.sceneDepth.value = depthTexture;
    if (depthPacking && depthPacking !== THREE.BasicDepthPacking) {
      throw new Error('Only BasicDepthPacking is supported');
    }
  }

  public updateUniforms(props: GodraysEffectProps, params: GodraysEffectParams): void {
    const { pointLight } = props;
    const pointLightShadow = pointLight.shadow;
    const depthCube = pointLightShadow?.map?.texture ?? null;
    const mapSize = pointLightShadow?.map?.height ?? 1;

    const uniforms = this.material.uniforms;
    uniforms.density.value = params.density;
    uniforms.maxDensity.value = params.maxDensity;
    uniforms.lightPos.value = pointLight.position;
    uniforms.cameraPos.value = props.camera.position;
    uniforms.projectionMatrixInv.value = props.camera.projectionMatrixInverse;
    uniforms.viewMatrixInv.value = props.camera.matrixWorld;
    uniforms.depthCube.value = depthCube;
    uniforms.mapSize.value = mapSize;
    uniforms.pointLightCameraNear.value = pointLightShadow?.camera.near ?? 0.1;
    uniforms.pointLightCameraFar.value = pointLightShadow?.camera.far ?? 1000;
    uniforms.density.value = params.density;
    uniforms.maxDensity.value = params.maxDensity;
    uniforms.distanceAttenuation.value = params.distanceAttenuation;
  }
}

interface GodraysCompositorMaterialProps {
  godrays: THREE.Texture;
  edgeStrength: number;
  edgeRadius: number;
  color: THREE.Color;
}

class GodraysCompositorMaterial extends THREE.ShaderMaterial implements Resizable {
  constructor(
    camera: THREE.PerspectiveCamera,
    { godrays, edgeStrength, edgeRadius, color }: GodraysCompositorMaterialProps
  ) {
    const uniforms = {
      cameraNear: { value: 0 },
      cameraFar: { value: 0 },
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

    this.updateUniforms(camera, edgeStrength, edgeRadius, color);
  }

  public updateUniforms(
    camera: THREE.PerspectiveCamera,
    edgeStrength: number,
    edgeRadius: number,
    color: THREE.Color
  ): void {
    this.uniforms.edgeStrength.value = edgeStrength;
    this.uniforms.edgeRadius.value = edgeRadius;
    this.uniforms.color.value = color;
  }

  setSize(width: number, height: number): void {
    this.uniforms.resolution.value.set(width, height);
  }
}

class GodraysCompositorPass extends Pass implements Resizable {
  constructor(camera: THREE.PerspectiveCamera, props: GodraysCompositorMaterialProps) {
    super('GodraysCompositorPass');
    this.fullscreenMaterial = new GodraysCompositorMaterial(camera, props);
  }

  public updateUniforms(camera: THREE.PerspectiveCamera, params: GodraysEffectParams): void {
    (this.fullscreenMaterial as GodraysCompositorMaterial).updateUniforms(
      camera,
      params.edgeStrength,
      params.edgeRadius,
      params.color
    );
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    (this.fullscreenMaterial as GodraysCompositorMaterial).uniforms.sceneDiffuse.value = inputBuffer.texture;
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    if (depthPacking && depthPacking !== THREE.BasicDepthPacking) {
      throw new Error('Only BasicDepthPacking is supported');
    }
    (this.fullscreenMaterial as GodraysCompositorMaterial).uniforms.sceneDepth.value = depthTexture;
  }

  setSize(width: number, height: number): void {
    (this.fullscreenMaterial as GodraysCompositorMaterial).setSize(width, height);
  }
}

interface GodraysEffectProps {
  pointLight: THREE.PointLight;
  camera: THREE.PerspectiveCamera;
  blueNoiseTexture: THREE.Texture;
}

export interface GodraysEffectParams {
  /**
   * The rate of accumulation for the godrays.  Higher values roughly equate to more humid air/denser fog.
   */
  density: number;
  /**
   * The maximum density of the godrays.  Limits the maximum brightness of the godrays.
   */
  maxDensity: number;
  /**
   * TODO: Document this
   */
  edgeStrength: number;
  /**
   * TODO: Document this
   */
  edgeRadius: number;
  /**
   * Higher values decrease the accumulation of godrays the further away they are from the light source.
   */
  distanceAttenuation: number;
  /**
   * The color of the godrays.
   */
  color: THREE.Color;
}

const defaultParams: GodraysEffectParams = {
  density: 1 / 128,
  maxDensity: 0.5,
  edgeStrength: 2,
  edgeRadius: 1,
  distanceAttenuation: 0.005,
  color: new THREE.Color(0xffffff),
};

const populateParams = (partialParams: Partial<GodraysEffectParams>): GodraysEffectParams => {
  return {
    ...defaultParams,
    ...partialParams,
    color: new THREE.Color(partialParams.color ?? defaultParams.color),
  };
};

export class GodraysEffect extends Pass implements Disposable {
  private props: GodraysEffectProps;

  private godraysRenderTarget = new THREE.WebGLRenderTarget(1, 1, {
    minFilter: THREE.LinearFilter,
    magFilter: THREE.LinearFilter,
    format: THREE.RGBAFormat,
  });
  private godraysPass: GodraysPass;

  private compositorPass: GodraysCompositorPass;

  /**
   * Constructs a new GodraysEffect.  Casts godrays from a point light source.  Add to your scene's composer like this:
   *
   * ```ts
   * import { EffectComposer, RenderPass } from 'postprocessing';
   *
   * const composer = new EffectComposer(renderer, { frameBufferType: THREE.HalfFloatType });
   * const renderPass = new RenderPass(scene, camera);
   * renderPass.renderToScreen = false;
   * composer.addPass(renderPass);
   *
   * const godraysEffect = new GodraysEffect(pointLight, camera, blueNoiseTexture);
   * godraysEffect.renderToScreen = true;
   * composer.addPass(godraysEffect);
   *
   * function animate() {
   *   composer.render(scene, camera);
   * }
   * ```
   *
   * @param light The light source to use for the godrays.
   * @param camera The camera used to render the scene.
   * @param blueNoiseTexture A texture containing blue noise.  This is used to dither the godrays to reduce banding.
   * @param partialParams The parameters to use for the godrays effect.  Will use default values for any parameters not specified.
   */
  constructor(
    light: THREE.PointLight,
    camera: THREE.PerspectiveCamera,
    blueNoiseTexture: THREE.Texture,
    partialParams: Partial<GodraysEffectParams> = {}
  ) {
    super('GodraysEffect');

    this.props = {
      camera,
      pointLight: light,
      blueNoiseTexture,
    };
    const params = populateParams(partialParams);

    this.godraysPass = new GodraysPass(this.props, params);
    this.godraysPass.needsDepthTexture = true;

    this.compositorPass = new GodraysCompositorPass(camera, {
      godrays: this.godraysRenderTarget.texture,
      edgeStrength: params.edgeStrength,
      edgeRadius: params.edgeRadius,
      color: params.color,
    });
    this.compositorPass.needsDepthTexture = true;

    // Indicate to the composer that this pass needs depth information from the previous pass
    this.needsDepthTexture = true;
  }

  /**
   * Updates the parameters used for the godrays effect.  Will use default values for any parameters not specified.
   */
  public setParams(partialParams: Partial<GodraysEffectParams>): void {
    const params = populateParams(partialParams);
    this.godraysPass.updateUniforms(this.props, params);
    this.compositorPass.updateUniforms(this.props.camera, params);
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    this.godraysPass.render(renderer, inputBuffer, this.godraysRenderTarget);

    this.compositorPass.render(renderer, inputBuffer, this.renderToScreen ? null : outputBuffer);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    this.godraysPass.setDepthTexture(depthTexture, depthPacking);
    this.compositorPass.setDepthTexture(depthTexture, depthPacking);
  }

  setSize(width: number, height: number): void {
    this.godraysRenderTarget.setSize(
      Math.ceil(width * GODRAYS_RESOLUTION_SCALE),
      Math.ceil(height * GODRAYS_RESOLUTION_SCALE)
    );
    this.godraysPass.setSize(width, height);
    this.compositorPass.setSize(width, height);
  }

  dispose(): void {
    this.godraysRenderTarget.dispose();
    this.godraysPass.dispose();
    this.compositorPass.dispose();
    super.dispose();
  }
}
