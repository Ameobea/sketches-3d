export const initWebSynth = () => {
  const content = document.createElement('div');
  content.id = 'content';
  content.style.display = 'none';
  document.body.appendChild(content);
  return import('https://ameo.dev/web-synth-headless/headless.js').then(async mod => {
    const webSynthHandle = await mod.initHeadlessWebSynth();
    console.log(webSynthHandle);
    return webSynthHandle;
  });
};
