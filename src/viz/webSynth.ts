export const initWebSynth = (args: { compositionIDToLoad?: number }) => {
  // const toPersist = ['globalVolume'];
  // const persisted = toPersist.reduce((acc, key) => {
  //   const val = localStorage.getItem(key);
  //   if (val !== null && val !== undefined) {
  //     acc[key] = val;
  //   }
  //   return acc;
  // }, {} as Record<string, any>);

  // localStorage.clear(); // Local storage belongs to web synth exclusively
  // Object.entries(persisted).forEach(([key, val]) => {
  //   localStorage.setItem(key, val);
  // });

  const content = document.createElement('div');
  content.id = 'content';
  content.style.display = 'none';
  document.body.appendChild(content);
  return import('https://ameo.dev/web-synth-headless/headless.js').then(async mod => {
    const webSynthHandle = await mod.initHeadlessWebSynth(args);
    return webSynthHandle;
  });
};

const ctx = new AudioContext();
export const setGlobalVolume = (volume: number) => {
  if (volume < 0 || volume > 100) {
    throw new Error('Volume must be between 0 and 100');
  }

  (ctx as any).globalVolume.gain.value = +volume / 100;
  localStorage.globalVolume = volume;
};
(window as any).setGlobalVolume = setGlobalVolume;
