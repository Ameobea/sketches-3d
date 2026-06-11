import { applyAudioSettings } from '.';
import { loadVizConfig } from './conf';

let ctx: AudioContext | null = null;

// Sets the music bus gain (the `globalVolume` node web-synth outputs into), not the game's master volume
export const setGlobalVolume = (volume: number) => {
  if (volume < 0 || volume > 100) {
    throw new Error('Volume must be between 0 and 100');
  }

  (ctx as any).globalVolume.gain.value = +volume / 100;
};

export const initWebSynth = (args: { compositionIDToLoad?: number }) => {
  if (!ctx) {
    ctx = new AudioContext();
    (window as any).setGlobalVolume = setGlobalVolume;
  }
  const content = document.createElement('div');
  content.id = 'content';
  content.style.display = 'none';
  document.body.appendChild(content);
  // This is the `web-synth-headless-test` phost deployment
  applyAudioSettings(loadVizConfig().audio);
  return import('https://ameo.dev/web-synth-headless/headless.js').then(async mod => {
    const webSynthHandle = await mod.initHeadlessWebSynth(args);
    applyAudioSettings(loadVizConfig().audio);
    return { ...webSynthHandle, setGlobalVolume };
  });
};
