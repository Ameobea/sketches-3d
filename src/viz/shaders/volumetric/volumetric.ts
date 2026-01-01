import { type Disposable, Pass } from 'postprocessing';
import * as THREE from 'three';

import { getBlueNoiseTexture } from './blueNoise';
import VolumetricFragmentShader from './volumetric.frag?raw';
import VolumetricVertexShader from './volumetric.vert?raw';
import {
  type VolumetricCompositorMaterial,
  VolumetricCompositorPass,
  type VolumetricPassCompositorParams,
} from './compositorPass';

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
   * Controls the compositing pass that upscales + combines the volumetric pass with the main scene.
   *
   * Only applies if `halfRes` is set to `true`.
   */
  compositor?: Partial<VolumetricPassCompositorParams>;
  /**
   * If set, the volumetric pass will render at half resolution and then upscale to full resolution.
   *
   * Cannot be changed after the pass is created.
   *
   * Default: false
   */
  halfRes?: boolean;
  globalScale?: number;
}

class VolumetricMaterial extends THREE.ShaderMaterial {
  constructor(params: VolumetricPassParams) {
    const uniforms = {
      sceneDepth: { value: null },
      sceneDiffuse: { value: null },
      blueNoise: { value: null },
      resolution: { value: new THREE.Vector2(1, 1) },
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
      blueNoiseResolution: { value: 256 },
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
    };

    super({
      name: 'VolumetricMaterial',
      uniforms,
      fragmentShader: VolumetricFragmentShader,
      vertexShader: VolumetricVertexShader,
      defines: params.halfRes ? {} : { DO_DIRECT_COMPOSITING: '1' },
    });

    getBlueNoiseTexture(new THREE.TextureLoader()).then(blueNoiseTexture => {
      this.uniforms.blueNoise.value = blueNoiseTexture;
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

  constructor(scene: THREE.Scene, camera: THREE.PerspectiveCamera, params: VolumetricPassParams) {
    super('VolumetricPass', undefined, new THREE.Camera());
    this.params = params;

    // Indicate to the composer that this pass needs depth information from the previous pass
    this.needsDepthTexture = true;

    this.playerCamera = camera;
    this.material = new VolumetricMaterial(params);
    this.fullscreenMaterial = this.material;
    if (params.halfRes) {
      this.fogRenderTarget = new THREE.WebGLRenderTarget(1, 1, {
        type: THREE.HalfFloatType,
      });
      this.compositorPass = new VolumetricCompositorPass({
        camera,
        params: params.compositor,
        fogTexture: this.fogRenderTarget.texture,
      });
    }

    this.ambientLight = scene.children.find(child => child instanceof THREE.AmbientLight) as
      | THREE.AmbientLight
      | undefined;

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
        this.fogRenderTarget!.texture;
      this.compositorPass.render(renderer, inputBuffer, this.renderToScreen ? null : outputBuffer);
    }
  }

  override setSize(width: number, height: number): void {
    this.material.uniforms.resolution.value.set(width, height);
    this.fogRenderTarget?.setSize(
      Math.ceil(width * (this.params.halfRes ? 0.5 : 1)),
      Math.ceil(height * (this.params.halfRes ? 0.5 : 1))
    );
    this.compositorPass?.setSize(width, height);
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
