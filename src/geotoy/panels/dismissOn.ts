/** Escape-only dismissal, for surfaces holding unsaved input: an outside click there is far
 *  more likely to be a misclick than a cancel, and discards what the user typed. */
export const dismissOnEscape = (close: () => void) => {
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') close();
  };
  window.addEventListener('keydown', onKeyDown);
  return () => window.removeEventListener('keydown', onKeyDown);
};

/**
 * Dismissal for popovers/menus: outside click or Escape. The click listener is deferred a
 * task so the very click that opened the surface can't immediately close it again, and binds
 * on `document` rather than `svelte:window` so it isn't racing the opening handler. Returns
 * the teardown, so callers use it as an `$effect` body and only while the surface is open.
 */
export const dismissOn = (insideSelector: string, close: () => void) => {
  const onDocClick = (e: MouseEvent) => {
    if (!(e.target as HTMLElement | null)?.closest?.(insideSelector)) close();
  };
  const timer = setTimeout(() => document.addEventListener('click', onDocClick));
  const stopEscape = dismissOnEscape(close);
  return () => {
    clearTimeout(timer);
    document.removeEventListener('click', onDocClick);
    stopEscape();
  };
};
