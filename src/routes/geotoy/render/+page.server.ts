// Transient render route — bypasses DB lookup. The composition payload is injected
// into the page by the renderer service (puppeteer evaluateOnNewDocument) and read
// from `window.__transientCompositionPayload` client-side.
export const ssr = false;
export const prerender = false;
