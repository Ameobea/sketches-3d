import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { glslFloat, type ParticlePipelineCtx } from 'src/viz/particles/particleMaterial';
import type { ParticleFrame } from 'src/viz/particles/ParticleSystem';
import { HijackedMRTPass } from 'src/viz/passes/hijackedMRTPass';
import { renderToTarget } from 'src/viz/util/renderToTarget';
import FOG_VERT from './shaders/final.vert?raw';

export interface ParticlePassOpts extends Omit<ParticlePipelineCtx, 'sceneDepth'> {
  /** Scene alpha already carries volumetric fog coverage; otherwise this pass zeroes it first. */
  sceneAlphaIsCoverage: boolean;
  /**
   * Whether the scene coverage also hides the emissive meshes + sky drawn before it. Off where
   * near-field bypass meshes that don't write depth (portals) must punch through fog the raymarch
   * integrated behind them.
   */
  coverageAttenuatesEmissive: boolean;
}

/** `softDepth` systems sample the scene depth, so they draw into a target that doesn't attach it. */
class DepthlessTarget extends HijackedMRTPass {
  bind(renderer: THREE.WebGLRenderer, colorRT: THREE.WebGLRenderTarget, emissiveRT: THREE.WebGLRenderTarget) {
    return this.bindAttachments(renderer, colorRT, emissiveRT);
  }
}

const FULLSCREEN_VERT = `
  varying vec2 vUv;
  void main() {
    vUv = position.xy * 0.5 + 0.5;
    gl_Position = vec4(position.xy, 1.0, 1.0);
  }`;

const buildCoverageMaterial = () =>
  new THREE.ShaderMaterial({
    uniforms: { tScene: { value: null } },
    vertexShader: FULLSCREEN_VERT,
    fragmentShader: `
      uniform sampler2D tScene;
      varying vec2 vUv;
      void main() { gl_FragColor = vec4(0.0, 0.0, 0.0, clamp(texture2D(tScene, vUv).a, 0.0, 1.0)); }`,
    depthTest: false,
    depthWrite: false,
    blending: THREE.CustomBlending,
    blendSrc: THREE.ZeroFactor,
    blendDst: THREE.OneMinusSrcAlphaFactor,
    blendSrcAlpha: THREE.ZeroFactor,
    blendDstAlpha: THREE.OneMinusSrcAlphaFactor,
  });

/**
 * Premultiplied "over" of a low-res particle layer, upsampled 2×2 with bilinear weights gated by
 * depth similarity so haze doesn't bleed across silhouettes; taps reference the same full-res depth
 * texel the low-res fragment tested against. Falls back to plain bilinear where every tap disagrees.
 */
const buildLowResComposite = (low: THREE.WebGLRenderTarget, depth: THREE.Texture, scale: number) =>
  new THREE.ShaderMaterial({
    uniforms: {
      tLow: { value: low.texture },
      tDepth: { value: depth },
      pNearFar: { value: new THREE.Vector2() },
    },
    defines: { INV_SCALE: glslFloat(1 / scale) },
    vertexShader: FULLSCREEN_VERT,
    fragmentShader: `
      uniform highp sampler2D tLow;
      uniform highp sampler2D tDepth;
      uniform vec2 pNearFar;
      float linZ(ivec2 t) {
        float d = texelFetch(tDepth, t, 0).r;
        return pNearFar.x * pNearFar.y / (pNearFar.y - d * (pNearFar.y - pNearFar.x));
      }
      void main() {
        float z = linZ(ivec2(gl_FragCoord.xy));
        vec2 lowPos = gl_FragCoord.xy / INV_SCALE - 0.5;
        ivec2 base = ivec2(floor(lowPos));
        vec2 f = lowPos - vec2(base);
        ivec2 lowMax = textureSize(tLow, 0) - 1;
        vec4 sum = vec4(0.0), bilinear = vec4(0.0);
        float wsum = 0.0;
        for (int j = 0; j < 2; j++) for (int i = 0; i < 2; i++) {
          ivec2 t = clamp(base + ivec2(i, j), ivec2(0), lowMax);
          vec4 c = texelFetch(tLow, t, 0);
          float wb = (i == 0 ? 1.0 - f.x : f.x) * (j == 0 ? 1.0 - f.y : f.y);
          float dz = (linZ(ivec2((vec2(t) + 0.5) * INV_SCALE)) - z) / z;
          float w = wb * exp(-dz * dz * 200.0);
          bilinear += c * wb;
          sum += c * w;
          wsum += w;
        }
        gl_FragColor = wsum > 1e-4 ? sum / wsum : bilinear;
      }`,
    depthTest: false,
    depthWrite: false,
    blending: THREE.CustomBlending,
    blendSrc: THREE.OneFactor,
    blendDst: THREE.OneMinusSrcAlphaFactor,
    blendSrcAlpha: THREE.OneFactor,
    blendDstAlpha: THREE.OneMinusSrcAlphaFactor,
  });

/**
 * Applies the scene's distance fog to the opaque frame before any particle is drawn, so particles
 * composite over an already-fogged background. FinalPass's coverage-attenuated fog is exact only
 * for opaque coverage; for low-alpha sprites it dims their own contribution by fog × (1 − alpha).
 */
const buildFogMaterial = (fogShader: string, sceneAlphaIsCoverage: boolean) =>
  new THREE.ShaderMaterial({
    defines: sceneAlphaIsCoverage ? { SCENE_ALPHA_IS_FOG_COVERAGE: '1' } : {},
    uniforms: {
      inputBuffer: { value: null },
      depthBuffer: { value: null },
      projectionMatrixInverse: { value: new THREE.Matrix4() },
      cameraWorldMatrix: { value: new THREE.Matrix4() },
      fogCameraPos: { value: new THREE.Vector3() },
      fogPlayerPos: { value: new THREE.Vector3() },
      curTimeSeconds: { value: 0 },
    },
    vertexShader: FOG_VERT,
    fragmentShader: `${fogShader}
      uniform sampler2D inputBuffer;
      uniform sampler2D depthBuffer;
      uniform mat4 projectionMatrixInverse;
      uniform mat4 cameraWorldMatrix;
      uniform vec3 fogCameraPos;
      uniform vec3 fogPlayerPos;
      uniform float curTimeSeconds;
      varying vec2 vUv;
      varying vec3 vWorldRay;
      void main() {
        vec4 color = texture2D(inputBuffer, vUv);
        float coverage = 0.0;
        #ifdef SCENE_ALPHA_IS_FOG_COVERAGE
        coverage = clamp(color.a, 0.0, 1.0);
        #endif
        #ifndef FOG_DISABLED
        float depth = texture2D(depthBuffer, vUv).r;
        float ndcZ = depth * 2.0 - 1.0;
        float invW = 1.0 / (projectionMatrixInverse[2][3] * ndcZ + projectionMatrixInverse[3][3]);
        vec3 worldPos = vWorldRay * invW + cameraWorldMatrix[3].xyz;
        vec4 fog = getFogEffect(worldPos, fogCameraPos, fogPlayerPos, depth, curTimeSeconds);
        color.rgb = mix(color.rgb, fog.rgb, fog.a * (1.0 - coverage));
        #endif
        gl_FragColor = vec4(color.rgb, coverage);
      }`,
    depthTest: false,
    depthWrite: false,
    blending: THREE.NoBlending,
  });

interface LowResGroup {
  rt: THREE.WebGLRenderTarget;
  scene: THREE.Scene;
  composite: THREE.ShaderMaterial;
}

/**
 * Runs after the middle passes and before bloom, in draw order = compositing order:
 *  1. scene-route particles into the scene buffer, pre-fogged, blending their alpha into its alpha
 *     as coverage (the VolumetricPass contract) so FinalPass doesn't re-fog them at the opaque
 *     depth behind;
 *  2. that coverage (volumetric fog + scene-route particles) multiplied into emissiveRT, hiding the
 *     emissive content under it from both the composite and the bloom;
 *  3. emissive-route particles into emissiveRT, over all of it.
 * `renderScale` systems draw between 1 and 2 into a low-res layer per scale that is composited
 * over the scene buffer; nothing is allocated or run for it unless a system opts in.
 * With a fog shader the pass first writes the distance-fogged frame into the output buffer and
 * draws everything there (swapping), so FinalPass must not fog the scene again.
 */
export class ParticlePass extends HijackedMRTPass {
  private readonly viz: Viz;
  private readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly opts: ParticlePassOpts;
  private ctx!: ParticlePipelineCtx;
  private readonly softTarget: DepthlessTarget;
  /** [hard, soft] scenes per output route. */
  private readonly scenes = {
    scene: [new THREE.Scene(), new THREE.Scene()],
    emissive: [new THREE.Scene(), new THREE.Scene()],
  };
  private readonly lowRes = new Map<number, LowResGroup>();
  private readonly coverage: THREE.ShaderMaterial | null;
  private readonly fog: THREE.ShaderMaterial | null;
  private readonly frame: ParticleFrame = { time: 0, playerPos: new THREE.Vector3(), viewportHeight: 1 };
  private readonly size = new THREE.Vector2();
  private width: number;
  private height: number;

  constructor(
    viz: Viz,
    width: number,
    height: number,
    emissiveRT: THREE.WebGLRenderTarget,
    opts: ParticlePassOpts
  ) {
    super('ParticlePass', width, height);
    this.viz = viz;
    this.emissiveRT = emissiveRT;
    this.opts = opts;
    this.width = width;
    this.height = height;
    this.softTarget = new DepthlessTarget('ParticleSoftTarget', width, height);
    this.coverage = opts.coverageAttenuatesEmissive ? buildCoverageMaterial() : null;
    this.fog = opts.fogShader ? buildFogMaterial(opts.fogShader, opts.sceneAlphaIsCoverage) : null;
    if (this.fog) {
      this.fog.uniforms.projectionMatrixInverse.value = viz.camera.projectionMatrixInverse;
      this.fog.uniforms.cameraWorldMatrix.value = viz.camera.matrixWorld;
      this.needsSwap = true;
    }
  }

  setStableDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.attachDepth(depthTexture);
    this.ctx = { ...this.opts, sceneDepth: depthTexture };
    if (this.fog) this.fog.uniforms.depthBuffer.value = depthTexture;
  }

  setFogEnabled(enabled: boolean): void {
    if (!this.fog) return;
    const isDisabled = this.fog.defines.FOG_DISABLED === '1';
    if (!enabled === isDisabled) return;
    if (enabled) delete this.fog.defines.FOG_DISABLED;
    else this.fog.defines.FOG_DISABLED = '1';
    this.fog.needsUpdate = true;
  }

  override setSize(width: number, height: number): void {
    super.setSize(width, height);
    this.width = width;
    this.height = height;
    this.softTarget.setSize(width, height);
    for (const [scale, g] of this.lowRes) g.rt.setSize(Math.floor(width * scale), Math.floor(height * scale));
  }

  override dispose(): void {
    this.softTarget.dispose();
    for (const g of this.lowRes.values()) {
      g.rt.dispose();
      g.composite.dispose();
    }
    super.dispose();
  }

  private lowResGroup(scale: number): LowResGroup {
    let g = this.lowRes.get(scale);
    if (!g) {
      const rt = new THREE.WebGLRenderTarget(
        Math.floor(this.width * scale),
        Math.floor(this.height * scale),
        {
          type: THREE.HalfFloatType,
          format: THREE.RGBAFormat,
          depthBuffer: false,
          minFilter: THREE.NearestFilter,
          magFilter: THREE.NearestFilter,
        }
      );
      g = { rt, scene: new THREE.Scene(), composite: buildLowResComposite(rt, this.ctx.sceneDepth, scale) };
      this.lowRes.set(scale, g);
    }
    return g;
  }

  private blit(
    renderer: THREE.WebGLRenderer,
    material: THREE.ShaderMaterial,
    target: THREE.WebGLRenderTarget
  ) {
    this.fullscreenMaterial = material;
    renderer.setRenderTarget(target);
    renderer.render(this.scene, this.camera);
  }

  private drawLowRes(renderer: THREE.WebGLRenderer, inputBuffer: THREE.WebGLRenderTarget) {
    const cam = this.viz.camera as THREE.PerspectiveCamera;
    for (const g of this.lowRes.values()) {
      if (!g.scene.children.length) continue;
      renderToTarget(renderer, g.rt, g.scene, cam, { clearColor: 0, clearAlpha: 0 });
      (g.composite.uniforms.pNearFar.value as THREE.Vector2).set(cam.near, cam.far);
      this.blit(renderer, g.composite, inputBuffer);
    }
  }

  private clearSceneAlpha(renderer: THREE.WebGLRenderer): void {
    const gl = renderer.getContext() as WebGL2RenderingContext;
    renderer.state.buffers.color.setMask(false);
    gl.colorMask(false, false, false, true);
    gl.clearBufferfv(gl.COLOR, 0, [0, 0, 0, 0]);
    renderer.state.buffers.color.setMask(true);
  }

  private draw(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    [hard, soft]: THREE.Scene[]
  ) {
    const { camera } = this.viz;
    if (hard.children.length && this.bindAttachments(renderer, inputBuffer, this.emissiveRT))
      renderer.render(hard, camera);
    if (soft.children.length && this.softTarget.bind(renderer, inputBuffer, this.emissiveRT))
      renderer.render(soft, camera);
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget
  ): void {
    const { fpCtx } = this.viz;
    this.frame.time = fpCtx?.getPhysicsTime() ?? this.viz.clock.getElapsedTime();
    const playerPos = fpCtx?.playerController.getPosition();
    if (playerPos) this.frame.playerPos.set(playerPos.x(), playerPos.y(), playerPos.z());
    this.frame.viewportHeight = renderer.getDrawingBufferSize(this.size).y;

    const savedAutoClear = renderer.autoClear;
    renderer.autoClear = false;
    let target = inputBuffer;
    if (this.fog) {
      const u = this.fog.uniforms;
      u.inputBuffer.value = inputBuffer.texture;
      (u.fogCameraPos.value as THREE.Vector3).setFromMatrixPosition(this.viz.camera.matrixWorld);
      (u.fogPlayerPos.value as THREE.Vector3).copy(this.frame.playerPos);
      u.curTimeSeconds.value = this.frame.time;
      this.blit(renderer, this.fog, outputBuffer);
      target = outputBuffer;
    }
    if (!this.bindAttachments(renderer, target, this.emissiveRT)) {
      renderer.autoClear = savedAutoClear;
      return;
    }
    if (!this.fog && !this.opts.sceneAlphaIsCoverage) this.clearSceneAlpha(renderer);

    for (const sys of this.viz.particleSystems) {
      if (!sys.ensureMaterial(this.ctx)) continue;
      const { renderScale, output, softDepth } = sys.def;
      const scene = renderScale
        ? this.lowResGroup(renderScale).scene
        : this.scenes[output ?? 'scene'][softDepth ? 1 : 0];
      if (sys.mesh.parent !== scene) scene.add(sys.mesh);
      sys.update(this.frame);
    }

    this.draw(renderer, target, this.scenes.scene);
    this.drawLowRes(renderer, target);
    const hasCoverage =
      this.opts.sceneAlphaIsCoverage ||
      this.scenes.scene.some(s => s.children.length) ||
      [...this.lowRes.values()].some(g => g.scene.children.length);
    if (this.coverage && hasCoverage) {
      this.coverage.uniforms.tScene.value = target.texture;
      this.blit(renderer, this.coverage, this.emissiveRT);
    }
    this.draw(renderer, target, this.scenes.emissive);
    renderer.autoClear = savedAutoClear;
  }
}
