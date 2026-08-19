/** Drag behaviour shared by the geoscript control sections; pairs with `controlSection.css`. */

/**
 * Drags the pointer's target along its parent element, reporting the pointer's clamped
 * [0, 1] position on that parent. `snapOnDown` reports the initial press too, which suits
 * click-to-set markers but not ones whose press doubles as a selection.
 */
export const dragAlongBar = (
  e: PointerEvent,
  onFrac: (frac: number) => void,
  onEnd: () => void,
  snapOnDown = false
) => {
  const marker = e.currentTarget as HTMLElement;
  const bar = marker.parentElement!;
  marker.setPointerCapture(e.pointerId);
  const onMove = (ev: PointerEvent) => {
    const r = bar.getBoundingClientRect();
    onFrac(Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)));
  };
  if (snapOnDown) onMove(e);
  const onUp = () => {
    marker.removeEventListener('pointermove', onMove);
    marker.removeEventListener('pointerup', onUp);
    marker.removeEventListener('pointercancel', onUp);
    onEnd();
  };
  marker.addEventListener('pointermove', onMove);
  marker.addEventListener('pointerup', onUp);
  marker.addEventListener('pointercancel', onUp);
};

/** Builds a canvas `use:` action that repaints via `draw` on mount and on every update. */
export const redrawOn =
  <T>(draw: (canvas: HTMLCanvasElement, arg: T) => void) =>
  (canvas: HTMLCanvasElement, arg: T) => {
    draw(canvas, arg);
    return { update: (next: T) => draw(canvas, next) };
  };
