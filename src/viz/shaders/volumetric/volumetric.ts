import { type Disposable, Pass } from 'postprocessing';
import * as THREE from 'three';

import VolumetricFragmentShader from './volumetric.frag?raw';
import VolumetricVertexShader from './volumetric.vert?raw';
import { type VolumetricCompositorMaterial, VolumetricCompositorPass } from './compositorPass';

export interface VolumetricPassParams {
  ambientLightColor?: THREE.Color;
  ambientLightIntensity?: number;
  fogMinY?: number;
  fogMaxY?: number;
  baseRaymarchStepCount?: number;
  maxRaymarchStepCount?: number;
  maxRayLength?: number;
  minStepLength?: number;
  maxDensity?: number;
  fogColorHighDensity?: THREE.Vector3;
  fogColorLowDensity?: THREE.Vector3;
  lightColor?: THREE.Vector3;
  lightIntensity?: number;
  lightFalloffDistance?: number;
  fogFadeOutPow?: number;
  fogFadeOutRangeY?: number;
  /**
   * Main control over the density of the fog.
   */
  fogDensityMultiplier?: number;
  heightFogStartY?: number;
  heightFogEndY?: number;
  heightFogFactor?: number;
  /**
   * This value is added to the summed raw noise octave samples before being normalized to [0, 1] and raised to `noisePow`.
   *
   * Default: 0.485
   */
  noiseBias?: number;
  /**
   * Normalized noise is raised to this power after sampling.  Higher values can produce results with more contrast.
   *
   * Default: 3
   */
  noisePow?: number;
  /**
   * Controls the speed of the noise movement to simulate wind.
   *
   * Default: new THREE.Vector2(1.2, 0.8)
   */
  noiseMovementPerSecond?: THREE.Vector2;
  /**
   * Accumulated density after full raymarching is completed is multiplied by this value before raising to `postDensityPow`.
   *
   * Default: 1.2
   */
  postDensityMultiplier?: number;
  /**
   * Accumulated density after full raymarching is completed is raised to this power after multiplying by `postDensityMultiplier`.
   *
   * Default: 1
   */
  postDensityPow?: number;
  /**
   * @deprecated Use `renderScale: 0.5` instead.
   *
   * If set, the volumetric pass will render at half resolution and then upscale to full resolution.
   * Cannot be changed after the pass is created.
   */
  halfRes?: boolean;
  /**
   * Render the volumetric pass at this fraction of the full resolution, then upscale with
   * joint bilateral upsampling.  E.g. 0.5 = half res, 0.25 = quarter res.
   *
   * Takes precedence over `halfRes`.  Cannot be changed after the pass is created.
   *
   * Default: 1 (full resolution)
   */
  renderScale?: number;
  globalScale?: number;
  /**
   * Directional light whose shadow map will be sampled at each raymarch step to darken
   * fog in shadowed regions.  Only directional lights are supported.
   *
   * The light must have `castShadow = true` and a configured shadow camera.
   * The shadow map texture is read each frame from `light.shadow.map`.
   */
  shadowLight?: THREE.DirectionalLight;
  /**
   * How strongly the shadow map darkens the fog.  0 = no effect, 1 = full shadow.
   *
   * Default: 0.5
   */
  shadowIntensity?: number;
  /**
   * Depth bias (in light-space world units) used to prevent self-shadowing artifacts
   * in the volume.
   *
   * Default: 0.05
   */
  shadowBias?: number;
  /**
   * Number of noise octaves sampled per raymarch step.  Fewer octaves = smoother fog with less
   * fine detail, but dramatically cheaper.  Use 2 for low-spec mode to avoid aliasing when
   * `baseRaymarchStepCount` is also low.
   *
   * Default: 6
   */
  octaveCount?: number;
  /**
   * Half-size of the JBU neighborhood in low-res texel units.
   * `0` → 2×2 (4 taps), `1` → 4×4 (16 taps), `2` → 6×6 (36 taps).
   * Increase for very low render scales (e.g. 0.25) to soften the pixel grid.
   *
   * Default: 1
   */
  jbuExtent?: number;
  /**
   * Spatial Gaussian sigma for the JBU, in low-res texel units.
   * Larger values blend more aggressively across neighboring texels.
   * Scale roughly proportional to `1 / renderScale` relative to the 0.5× baseline of 1.8.
   *
   * Default: 1.8
   */
  jbuSpatialSigma?: number;
  /**
   * Depth Gaussian sigma for the JBU, as a fraction of the pixel's linearized depth.
   * Lower values make depth-edge preservation stricter, reducing fog bleed across edges.
   *
   * Default: 0.034
   */
  jbuDepthSigma?: number;
}

class VolumetricMaterial extends THREE.ShaderMaterial {
  constructor(params: VolumetricPassParams) {
    const uniforms = {
      sceneDepth: { value: null },
      sceneDiffuse: { value: null },
      cameraPos: { value: new THREE.Vector3(0, 0, 0) },
      cameraProjectionMatrixInv: { value: new THREE.Matrix4() },
      cameraMatrixWorld: { value: new THREE.Matrix4() },
      curTimeSeconds: { value: 0 },
      // lighting
      ambientLightColor: { value: new THREE.Color(0xffffff) },
      ambientLightIntensity: { value: 0 },
      // params
      fogMinY: { value: -40.0 },
      fogMaxY: { value: 4.4 },
      baseRaymarchStepCount: { value: 80 },
      maxRaymarchStepCount: { value: 400 },
      maxRayLength: { value: 300.0 },
      minStepLength: { value: 0.2 },
      maxDensity: { value: 1 },
      fogColorHighDensity: { value: new THREE.Vector3(0.06, 0.87, 0.53) },
      fogColorLowDensity: { value: new THREE.Vector3(0.11, 0.31, 0.7) },
      lightColor: { value: new THREE.Vector3(1.0, 0.0, 0.76) },
      lightIntensity: { value: 7.5 },
      lightFalloffDistance: { value: 110 },
      fogFadeOutPow: { value: 1 },
      fogFadeOutRangeY: { value: 1.5 },
      fogDensityMultiplier: { value: 0.086 },
      heightFogStartY: { value: -10 },
      heightFogEndY: { value: 8 },
      heightFogFactor: { value: 0.1852 },
      noiseBias: { value: 0.485 },
      noisePow: { value: 3 },
      noiseMovementPerSecond: { value: new THREE.Vector2(1.2, 0.8) },
      postDensityMultiplier: { value: 1.2 },
      postDensityPow: { value: 1 },
      noiseTexture: { value: null },
      globalScale: { value: 1 },
      // shadow map uniforms (only used when USE_SHADOW_MAP is defined)
      shadowMap: { value: null },
      shadowMatrix: { value: new THREE.Matrix4() },
      shadowCameraNear: { value: 0.1 },
      shadowCameraFar: { value: 500 },
      shadowIntensity: { value: 0.5 },
      shadowBias: { value: 0.05 },
    };

    super({
      name: 'VolumetricMaterial',
      uniforms,
      fragmentShader: VolumetricFragmentShader,
      vertexShader: VolumetricVertexShader,
      defines: {
        ...((params.renderScale ?? (params.halfRes ? 0.5 : 1)) < 1
          ? { NEEDS_COMPOSITING: '1' }
          : { DO_DIRECT_COMPOSITING: '1' }),
        OCTAVE_COUNT: String(params.octaveCount ?? 6),
        ...(params.shadowLight ? { USE_SHADOW_MAP: '1' } : {}),
      },
    });
  }
}

export class VolumetricPass extends Pass implements Disposable {
  /**
   * The camera used to render the main scene, different from the camera used to render the volumetric pass
   */
  private playerCamera: THREE.PerspectiveCamera = new THREE.PerspectiveCamera();
  private material: VolumetricMaterial;
  private curTimeSeconds = 0;
  private ambientLight?: THREE.AmbientLight;
  private params: VolumetricPassParams;
  private compositorPass?: VolumetricCompositorPass;
  private fogRenderTarget: THREE.WebGLRenderTarget | null = null;
  private noiseTexture3D: THREE.Data3DTexture;
  private renderScale: number;
  private shadowLight?: THREE.DirectionalLight;

  constructor(scene: THREE.Scene, camera: THREE.PerspectiveCamera, params: VolumetricPassParams) {
    super('VolumetricPass', undefined, new THREE.Camera());
    this.params = params;
    this.renderScale = params.renderScale ?? (params.halfRes ? 0.5 : 1);

    // Indicate to the composer that this pass needs depth information from the previous pass
    this.needsDepthTexture = true;

    this.playerCamera = camera;
    this.material = new VolumetricMaterial(params);
    this.fullscreenMaterial = this.material;
    if (this.renderScale < 1) {
      this.fogRenderTarget = new THREE.WebGLRenderTarget(1, 1, {
        count: 2,
        type: THREE.HalfFloatType,
      });
      // textures[0]: fog color (rgb) + density (a) — inherits defaults above
      // textures[1]: raw scene depth (r channel), nearest filter so no depth interpolation
      this.fogRenderTarget.textures[1].format = THREE.RedFormat;
      this.fogRenderTarget.textures[1].minFilter = THREE.NearestFilter;
      this.fogRenderTarget.textures[1].magFilter = THREE.NearestFilter;
      this.compositorPass = new VolumetricCompositorPass({
        camera,
        fogTexture: this.fogRenderTarget.textures[0],
        fogDepthTexture: this.fogRenderTarget.textures[1],
        jbuExtent: params.jbuExtent,
        jbuSpatialSigma: params.jbuSpatialSigma,
        jbuDepthSigma: params.jbuDepthSigma,
      });
    }

    this.ambientLight = scene.children.find(child => child instanceof THREE.AmbientLight) as
      | THREE.AmbientLight
      | undefined;
    this.shadowLight = params.shadowLight;

    this.updateUniforms();

    const noise = new Uint8Array(64 * 64 * 64);
    for (let i = 0; i < noise.length; i++) {
      noise[i] = Math.random() * 255;
    }
    this.noiseTexture3D = new THREE.Data3DTexture(noise, 64, 64, 64);
    this.noiseTexture3D.format = THREE.RedFormat;
    this.noiseTexture3D.type = THREE.UnsignedByteType;
    this.noiseTexture3D.wrapR = THREE.RepeatWrapping;
    this.noiseTexture3D.wrapS = THREE.RepeatWrapping;
    this.noiseTexture3D.wrapT = THREE.RepeatWrapping;
    this.noiseTexture3D.generateMipmaps = true;
    this.noiseTexture3D.minFilter = THREE.LinearMipmapLinearFilter;
    this.noiseTexture3D.magFilter = THREE.LinearFilter;
    this.noiseTexture3D.needsUpdate = true;
  }

  public setCurTimeSeconds(newCurTimeSeconds: number) {
    this.curTimeSeconds = newCurTimeSeconds;
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    this.updateUniforms();
    this.material.uniforms.sceneDiffuse.value = inputBuffer.texture;
    this.material.uniforms.curTimeSeconds.value = this.curTimeSeconds;

    renderer.setRenderTarget(
      (() => {
        if (this.compositorPass) {
          return this.fogRenderTarget!;
        }

        if (this.renderToScreen) {
          return null;
        }

        return outputBuffer;
      })()
    );
    renderer.render(this.scene, this.camera);

    if (this.compositorPass) {
      (this.compositorPass.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.fogTexture.value =
        this.fogRenderTarget!.textures[0];
      this.compositorPass.render(renderer, inputBuffer, this.renderToScreen ? null : outputBuffer);
    }
  }

  override setSize(width: number, height: number): void {
    const fogWidth = Math.ceil(width * this.renderScale);
    const fogHeight = Math.ceil(height * this.renderScale);
    this.fogRenderTarget?.setSize(fogWidth, fogHeight);
    this.compositorPass?.setSize(width, height);
    this.compositorPass?.setFogResolution(fogWidth, fogHeight);
  }

  override setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    this.material.uniforms.sceneDepth.value = depthTexture;
    this.compositorPass?.setDepthTexture(depthTexture, depthPacking);
  }

  public updateUniforms(): void {
    this.material.uniforms.cameraPos.value = this.playerCamera.position;
    this.material.uniforms.cameraProjectionMatrixInv.value = this.playerCamera.projectionMatrixInverse;
    this.material.uniforms.cameraMatrixWorld.value = this.playerCamera.matrixWorld;

    this.material.uniforms.ambientLightColor.value =
      this.params.ambientLightColor ?? this.ambientLight?.color ?? new THREE.Color(0xffffff);
    this.material.uniforms.ambientLightIntensity.value =
      this.params.ambientLightIntensity ?? this.ambientLight?.intensity ?? 0;
    this.material.uniforms.noiseTexture.value = this.noiseTexture3D;

    if (this.shadowLight?.shadow?.map) {
      const shadow = this.shadowLight.shadow;
      this.material.uniforms.shadowMap.value = shadow.map!.texture;
      this.material.uniforms.shadowMatrix.value
        .copy(shadow.camera.projectionMatrix)
        .multiply(shadow.camera.matrixWorldInverse);
      this.material.uniforms.shadowCameraNear.value = shadow.camera.near;
      this.material.uniforms.shadowCameraFar.value = shadow.camera.far;
    }

    for (const [key, value] of Object.entries(this.params)) {
      if (
        value === null ||
        value === undefined ||
        !(key in this.material.uniforms) ||
        key === 'ambientLightColor' ||
        key === 'ambientLightIntensity'
      ) {
        continue;
      }

      (this.material.uniforms as any)[key].value = value;
    }
  }

  override dispose(): void {
    this.fogRenderTarget?.dispose();
    this.compositorPass?.dispose();
    super.dispose();
  }
}
