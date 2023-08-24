export const createStatsContainer = (topPx: number): HTMLDivElement => {
  const posDisplayElem = document.createElement('div');
  posDisplayElem.style.position = 'absolute';
  posDisplayElem.style.right = '0px';
  posDisplayElem.style.color = 'white';
  posDisplayElem.style.fontSize = '12px';
  posDisplayElem.style.fontFamily = 'monospace';
  posDisplayElem.style.padding = '4px';
  posDisplayElem.style.backgroundColor = 'rgba(0, 0, 0, 0.5)';
  posDisplayElem.style.zIndex = '1';
  posDisplayElem.style.top = `${topPx}px`;
  return posDisplayElem;
};
