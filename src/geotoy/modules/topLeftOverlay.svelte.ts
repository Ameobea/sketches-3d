import { SvelteMap } from 'svelte/reactivity';

/**
 * The fixed overlay column in the viewport's top-left corner. Which overlays occupy it changes
 * with the mode, and their heights change with run state — the FPS meter in mesh mode, either
 * texture HUD in texture mode (taller with an output chip row, a stack-index slider or a preview
 * note), then the input controls — so none of them can hard-code an offset against its neighbours.
 * Each publishes its measured height under a slot and takes its own `top` from `topLeftOffset`; a
 * new overlay only needs a slot number.
 */
export const TopLeftSlot = { stats: 0, hud: 10, controls: 20 } as const;

const COLUMN_TOP = 6;
const GAP = 6;

const heights = new SvelteMap<number, number>();

/** Publishes a slot's measured height; returns the teardown, for use as an `$effect` body. */
export const setTopLeftSlot = (slot: number, height: number) => {
  heights.set(slot, height);
  return () => heights.delete(slot);
};

/** `top` for a slot, clearing every occupied slot above it. */
export const topLeftOffset = (slot: number): number => {
  let top = COLUMN_TOP;
  for (const [s, h] of heights) {
    if (s < slot && h > 0) {
      top += h + GAP;
    }
  }
  return top;
};
