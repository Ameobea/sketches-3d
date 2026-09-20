import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { ParticlePipelineCtx } from 'src/viz/particles/particleMaterial';
import type { ParticleFrame } from 'src/viz/particles/ParticleSystem';
import { HijackedMRTPass } from 'src/viz/passes/hijackedMRTPass';

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

const buildCoverageMaterial = () =>
  new THREE.ShaderMaterial({
    uniforms: { tScene: { value: null } },
    vertexShader: `
      varying vec2 vUv;
      void main() {
        vUv = position.xy * 0.5 + 0.5;
        gl_Position = vec4(position.xy, 1.0, 1.0);
      }`,
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
 * Runs after the middle passes and before bloom, in draw order = compositing order:
 *  1. scene-route particles into the scene buffer, pre-fogged, blending their alpha into its alpha
 *     as coverage (the VolumetricPass contract) so FinalPass doesn't re-fog them at the opaque
 *     depth behind;
 *  2. that coverage (volumetric fog + scene-route particles) multiplied into emissiveRT, hiding the
 *     emissive content under it from both the composite and the bloom;
 *  3. emissive-route particles into emissiveRT, over all of it.
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
  private readonly frame: ParticleFrame = { time: 0, playerPos: new THREE.Vector3(), viewportHeight: 1 };
  private readonly size = new THREE.Vector2();

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
    this.softTarget = new DepthlessTarget('ParticleSoftTarget', width, height);
    if (opts.coverageAttenuatesEmissive) this.fullscreenMaterial = buildCoverageMaterial();
  }

  setStableDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.attachDepth(depthTexture);
    this.ctx = { ...this.opts, sceneDepth: depthTexture };
  }

  override setSize(width: number, height: number): void {
    super.setSize(width, height);
    this.softTarget.setSize(width, height);
  }

  override dispose(): void {
    this.softTarget.dispose();
    super.dispose();
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

  override render(renderer: THREE.WebGLRenderer, inputBuffer: THREE.WebGLRenderTarget): void {
    if (!this.bindAttachments(renderer, inputBuffer, this.emissiveRT)) return;
    if (!this.opts.sceneAlphaIsCoverage) this.clearSceneAlpha(renderer);

    const { fpCtx } = this.viz;
    this.frame.time = fpCtx?.getPhysicsTime() ?? this.viz.clock.getElapsedTime();
    const playerPos = fpCtx?.playerController.getPosition();
    if (playerPos) this.frame.playerPos.set(playerPos.x(), playerPos.y(), playerPos.z());
    this.frame.viewportHeight = renderer.getDrawingBufferSize(this.size).y;

    for (const sys of this.viz.particleSystems) {
      if (!sys.ensureMaterial(this.ctx)) continue;
      const scene = this.scenes[sys.def.output ?? 'scene'][sys.def.softDepth ? 1 : 0];
      if (sys.mesh.parent !== scene) scene.add(sys.mesh);
      sys.update(this.frame);
    }

    const savedAutoClear = renderer.autoClear;
    renderer.autoClear = false;
    this.draw(renderer, inputBuffer, this.scenes.scene);
    const hasCoverage = this.opts.sceneAlphaIsCoverage || this.scenes.scene.some(s => s.children.length);
    if (this.opts.coverageAttenuatesEmissive && hasCoverage) {
      (this.fullscreenMaterial as THREE.ShaderMaterial).uniforms.tScene.value = inputBuffer.texture;
      renderer.setRenderTarget(this.emissiveRT);
      renderer.render(this.scene, this.camera);
    }
    this.draw(renderer, inputBuffer, this.scenes.emissive);
    renderer.autoClear = savedAutoClear;
  }
}
