import { type Disposable, Pass } from 'postprocessing';
import * as THREE from 'three';

import { getBlueNoiseTexture } from './blueNoise';
import VolumetricFragmentShader from './volumetric.frag?raw';
import VolumetricVertexShader from './volumetric.vert?raw';

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
  fogDensityMultiplier?: number;
  heightFogStartY?: number;
  heightFogEndY?: number;
  heightFogFactor?: number;
  noiseBias?: number;
  noiseRotationPerSecond?: number;
  noiseMovementPerSecond?: THREE.Vector2;
  postDensityMultiplier?: number;
  postDensityPow?: number;
}

class VolumetricMaterial extends THREE.ShaderMaterial {
  constructor() {
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
      fogFadeOutPow: { value: 2 },
      fogFadeOutRangeY: { value: 1.5 },
      fogDensityMultiplier: { value: 0.086 },
      heightFogStartY: { value: -10 },
      heightFogEndY: { value: 8 },
      heightFogFactor: { value: 0.1852 },
      noiseBias: { value: 0.485 },
      noiseRotationPerSecond: { value: 0.3 },
      noiseMovementPerSecond: { value: new THREE.Vector2(1.2, 0.8) },
      postDensityMultiplier: { value: 1.2 },
      postDensityPow: { value: 1 },
    };

    super({
      name: 'VolumetricMaterial',
      uniforms,
      fragmentShader: VolumetricFragmentShader,
      vertexShader: VolumetricVertexShader,
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
  private material: VolumetricMaterial = new VolumetricMaterial();
  private curTimeSeconds = 0;
  private ambientLight?: THREE.AmbientLight;
  private params: VolumetricPassParams;

  constructor(scene: THREE.Scene, camera: THREE.PerspectiveCamera, params: VolumetricPassParams) {
    super('VolumetricPass');
    this.params = params;

    // Indicate to the composer that this pass needs depth information from the previous pass
    this.needsDepthTexture = true;

    this.playerCamera = camera;
    this.material = new VolumetricMaterial();
    this.fullscreenMaterial = this.material;

    this.ambientLight = scene.children.find(child => child instanceof THREE.AmbientLight) as
      | THREE.AmbientLight
      | undefined;

    this.updateUniforms();
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
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
  }

  override setSize(width: number, height: number): void {
    this.material.uniforms.resolution.value.set(width, height);
  }

  override setDepthTexture(
    depthTexture: THREE.Texture,
    _depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    this.material.uniforms.sceneDepth.value = depthTexture;
  }

  public updateUniforms(): void {
    this.material.uniforms.cameraPos.value = this.playerCamera.position;
    this.material.uniforms.cameraProjectionMatrixInv.value = this.playerCamera.projectionMatrixInverse;
    this.material.uniforms.cameraMatrixWorld.value = this.playerCamera.matrixWorld;

    this.material.uniforms.ambientLightColor.value =
      this.ambientLight?.color ?? this.params.ambientLightColor ?? new THREE.Color(0xffffff);
    this.material.uniforms.ambientLightIntensity.value =
      this.ambientLight?.intensity ?? this.params.ambientLightIntensity ?? 0;

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
}
