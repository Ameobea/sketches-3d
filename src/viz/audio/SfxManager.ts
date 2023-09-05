import type { MaterialClass } from '../shaders/customShader';

export class SfxManager {
  private ctx: AudioContext;
  // TODO: implement this properly
  private landSound: AudioBuffer | null = null;
  private filterNode: BiquadFilterNode;

  constructor() {
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
  }

  public onPlayerLand(materialClass: MaterialClass) {
    // TODO: Implement properly
    if (this.landSound) {
      const source = this.ctx.createBufferSource();
      source.buffer = this.landSound;
      source.connect(this.filterNode);
      source.start();
    }
  }
}
