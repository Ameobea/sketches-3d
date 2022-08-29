export const initWebSynth = (args: { compositionIDToLoad?: number }) => {
  localStorage.clear(); // Local storage belongs to web synth exclusively
  const content = document.createElement('div');
  content.id = 'content';
  content.style.display = 'none';
  document.body.appendChild(content);
  return import('https://ameo.dev/web-synth-headless/headless.js').then(async mod => {
    const webSynthHandle = await mod.initHeadlessWebSynth(args);
    return webSynthHandle;
  });
};
