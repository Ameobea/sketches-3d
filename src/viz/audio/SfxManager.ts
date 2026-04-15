import type { Readable } from 'svelte/store';

import type { VizConfig } from '../conf';
import { MaterialClass } from '../shaders/customShader';

export interface SfxWalkConfig {
  playWalkSound?: (mat: MaterialClass) => void;
  timeBetweenStepsSeconds: number;
  timeBetweenStepsJitterSeconds: number;
}

export interface SfxLandConfig {
  /**
   * Per-material landing sound overrides.
   *
   * - `() => void` — fully custom callback (e.g. Web Synth MIDI).
   * - `string[]`   — list of sfx names (keys in SFX_DEFS) to pick from at random.
   *                  The scene is responsible for including them in `neededSfx`.
   *
   * When an override is present for a material class, the built-in default for that
   * class is skipped (and its sfx will not be fetched).
   */
  materialLandSounds: Partial<Record<MaterialClass, (() => void) | string[]>>;
}

export interface SfxConfig {
  walk: SfxWalkConfig;
  land?: Partial<SfxLandConfig>;
  neededSfx?: string[];
}

export const buildDefaultSfxConfig = (): SfxConfig => ({
  walk: {
    timeBetweenStepsSeconds: 0.35,
    timeBetweenStepsJitterSeconds: 0.04,
  },
});

const SFX_DEFS: Record<string, { url: string; playbackRate?: number }> = {
  dash: { url: 'https://i.ameo.link/cta.ogg' },
  dash_pickup: { url: 'https://i.ameo.link/ctb.ogg', playbackRate: 1.4 },
  land_default: { url: 'https://i.ameo.link/cvk.ogg' }, // original unfiltered: https://i.ameo.link/bga.mp3
  player_die: { url: 'https://i.ameo.link/cx8.ogg' },
  metal_plate_land_0: { url: 'https://i.ameo.link/dmi.ogg' },
  metal_plate_land_1: { url: 'https://i.ameo.link/dmj.ogg' },
  jump_pad_trigger: { url: 'https://i.ameo.link/dml.ogg' },
};

const METAL_PLATE_LAND_SFX = ['metal_plate_land_0', 'metal_plate_land_1'] as const;

export class SfxManager {
  private vizConfig: Readable<VizConfig> | null = null;
  private config: SfxConfig;
  private ctx: AudioContext;
  private loadedSfx: Set<string> = new Set();
  private registeredSfx: Map<string, AudioBuffer> = new Map();

  private curWalkMatClass: MaterialClass | null = null;
  private timeSinceLastStepSound = 0;
  private nextStepSoundTime = 0;

  private GlobalVolumeNode: GainNode;
  private sfxGainNode: GainNode;

  private get isWalking(): boolean {
    return this.curWalkMatClass !== null;
  }

  constructor(config?: SfxConfig) {
    this.config = config ?? buildDefaultSfxConfig();
    this.ctx = new AudioContext();

    this.GlobalVolumeNode = (this.ctx as any).globalVolume as GainNode;

    this.sfxGainNode = this.ctx.createGain();
    this.sfxGainNode.gain.value = 0.5;
    this.sfxGainNode.connect(this.GlobalVolumeNode);

    this.init();

    this.nextStepSoundTime = this.getNextStepSoundTime();
  }

  private async init() {
    this.loadNeededSfx();
  }

  private loadNeededSfx() {
    this.loadSfx('land_default');

    if (!this.config.neededSfx) {
      return;
    }

    for (const name of this.config.neededSfx) {
      this.loadSfx(name);
    }
  }

  public loadSfx(name: string) {
    if (this.loadedSfx.has(name)) {
      return;
    }

    const def = SFX_DEFS[name];
    if (!def) {
      console.error('SFX def not found:', name);
      return;
    }

    this.loadedSfx.add(name);
    fetch(def.url)
      .then(res => res.arrayBuffer())
      .then(buf => this.ctx.decodeAudioData(buf))
      .then(buf => this.registeredSfx.set(name, buf));
  }

  public setConfig(config: SfxConfig) {
    this.config = config;
    this.loadNeededSfx();
  }

  public setVizConfig(vizConfig: Readable<VizConfig>) {
    this.vizConfig = vizConfig;
    this.vizConfig.subscribe(config => {
      this.sfxGainNode.gain.value = config.audio.sfxVolume;
    });
  }

  public onPlayerLand(materialClass: MaterialClass) {
    const override = this.config.land?.materialLandSounds?.[materialClass];
    if (override !== undefined) {
      if (typeof override === 'function') {
        override();
      } else {
        this.playSfxRandom(override);
      }
      return;
    }

    if (materialClass === MaterialClass.MetalPlate) {
      this.playSfxRandom(METAL_PLATE_LAND_SFX);
      return;
    }

    this.playSfx('land_default');
  }

  /**
   * Called when a collision object with the given material class is registered in the scene.
   * Triggers lazy loading of any default sfx associated with that class, unless the scene has
   * already provided an override for it.
   */
  public onMaterialClassPresent(materialClass: MaterialClass) {
    if (materialClass === MaterialClass.MetalPlate) {
      const hasOverride = this.config.land?.materialLandSounds?.[materialClass] !== undefined;
      if (!hasOverride) {
        for (const name of METAL_PLATE_LAND_SFX) {
          this.loadSfx(name);
        }
      }
    }
  }

  /**
   * Called when a jump pad is registered in the scene. Triggers lazy loading of the
   * default jump pad trigger sfx.
   */
  public onJumpPadPresent() {
    this.loadSfx('jump_pad_trigger');
  }

  public onWalkStart(materialClass: MaterialClass) {
    this.curWalkMatClass = materialClass;
  }

  public onWalkStop() {
    this.curWalkMatClass = null;
  }

  public playSfxRandom(names: readonly string[]) {
    this.playSfx(names[Math.floor(Math.random() * names.length)]);
  }

  public playSfx(name: string) {
    const sound = this.registeredSfx.get(name);
    if (!sound) {
      console.warn('Sound not found:', name);
      return;
    }

    const source = this.ctx.createBufferSource();
    const def = SFX_DEFS[name];
    if (def?.playbackRate) {
      source.playbackRate.value = def.playbackRate;
    }
    source.buffer = sound;
    source.connect(this.sfxGainNode);
    source.start();
  }

  private getNextStepSoundTime() {
    let nextStepSoundTime = this.config.walk.timeBetweenStepsSeconds;
    nextStepSoundTime += Math.random() * this.config.walk.timeBetweenStepsJitterSeconds;
    nextStepSoundTime = Math.max(0, nextStepSoundTime);
    return nextStepSoundTime;
  }

  public tick(timeDiffSeconds: number, _curTimeSeconds: number) {
    if (this.config.walk.playWalkSound && this.isWalking) {
      this.timeSinceLastStepSound += timeDiffSeconds;
      if (this.timeSinceLastStepSound > this.nextStepSoundTime) {
        this.config.walk.playWalkSound(this.curWalkMatClass!);
        const remainder = Math.abs(this.timeSinceLastStepSound - this.nextStepSoundTime);
        this.timeSinceLastStepSound = remainder;
        this.nextStepSoundTime = this.getNextStepSoundTime();
      }
    }
  }
}
