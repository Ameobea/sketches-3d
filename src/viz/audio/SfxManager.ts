import type { MaterialClass } from '../shaders/customShader';

export class SfxManager {
  private ctx: AudioContext;
  // TODO: implement this properly
  private landSound: AudioBuffer | null = null;

  constructor() {
    this.ctx = new AudioContext();
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
      source.connect(this.ctx.destination);
      source.start();
    }
  }
}
