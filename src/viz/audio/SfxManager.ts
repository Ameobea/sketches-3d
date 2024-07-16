import type { MaterialClass } from '../shaders/customShader';

export interface SfxWalkConfig {
  playWalkSound?: (mat: MaterialClass) => void;
  timeBetweenStepsSeconds: number;
  timeBetweenStepsJitterSeconds: number;
}

export interface SfxLandConfig {
  materialLandSounds: Partial<Record<MaterialClass, () => void>>;
}

export interface SfxConfig {
  walk: SfxWalkConfig;
  land?: Partial<SfxLandConfig>;
}

export const buildDefaultSfxConfig = (): SfxConfig => ({
  walk: {
    timeBetweenStepsSeconds: 0.35,
    timeBetweenStepsJitterSeconds: 0.04,
  },
});

export class SfxManager {
  private config: SfxConfig;
  private ctx: AudioContext;
  private landSound: AudioBuffer | null = null;
  private filterNode: BiquadFilterNode;

  private curWalkMatClass: MaterialClass | null = null;
  private timeSinceLastStepSound = 0;
  private nextStepSoundTime = 0;

  private get isWalking(): boolean {
    return this.curWalkMatClass !== null;
  }

  constructor(config: SfxConfig) {
    this.config = config;
    this.ctx = new AudioContext();
    this.filterNode = this.ctx.createBiquadFilter();
    this.filterNode.type = 'lowpass';
    this.filterNode.frequency.value = 500;
    const GlobalVolumeNode = (this.ctx as any).globalVolume as GainNode;
    this.filterNode.connect(GlobalVolumeNode);
    const landSoundURL = 'https://i.ameo.link/bga.mp3';
    fetch(landSoundURL)
      .then(res => res.arrayBuffer())
      .then(buf => this.ctx.decodeAudioData(buf))
      .then(buf => {
        this.landSound = buf;
      });

    this.nextStepSoundTime = this.getNextStepSoundTime();
  }

  public onPlayerLand(materialClass: MaterialClass) {
    const customCb = this.config.land?.materialLandSounds?.[materialClass];
    if (customCb) {
      customCb();
      return;
    }

    if (this.landSound) {
      const source = this.ctx.createBufferSource();
      source.buffer = this.landSound;
      source.connect(this.filterNode);
      source.start();
    }
  }

  public onWalkStart(materialClass: MaterialClass) {
    this.curWalkMatClass = materialClass;
  }

  public onWalkStop() {
    this.curWalkMatClass = null;
  }

  private getNextStepSoundTime() {
    let nextStepSoundTime = this.config.walk.timeBetweenStepsSeconds;
    nextStepSoundTime += Math.random() * this.config.walk.timeBetweenStepsJitterSeconds;
    nextStepSoundTime = Math.max(0, nextStepSoundTime);
    return nextStepSoundTime;
  }

  public tick(timeDiffSeconds: number, curTimeSeconds: number) {
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
