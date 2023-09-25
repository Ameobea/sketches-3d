import { type Disposable, Pass } from 'postprocessing';
import * as THREE from 'three';

import { getBlueNoiseTexture } from './blueNoise';
import VolumetricFragmentShader from './volumetric.frag?raw';
import VolumetricVertexShader from './volumetric.vert?raw';

export interface VolumetricPassParams {
  ambientLightColor?: THREE.Color;
  ambientLightIntensity?: number;
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

      ambientLightColor: { value: new THREE.Color(0xffffff) },
      ambientLightIntensity: { value: 0 },
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
    console.log(this.ambientLight, scene.children);

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
  }
}
