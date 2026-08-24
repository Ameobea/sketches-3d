/**
 * three's `Stats` with its loop assumptions replaced: FPS is measured across consecutive
 * *presented* frames, so a governed loop that renders 4 frames then idles reads 60, not 4.
 * The panels hold that reading while idle, marked by a dim corner dot.
 */

// Below the governor's 250ms settle window: a change-driven burst is all the continuous
// frames there are, and a longer window would never close before the loop went idle.
const FPS_WINDOW_MS = 150;
const FPS_WINDOW_MIN_FRAMES = 3;
/** A longer gap than any live frame is idle time, not render time. */
const IDLE_GAP_MS = 250;

import Stats from 'three/examples/jsm/libs/stats.module.js';

import type { FrameGovernorTier } from 'src/viz/frameGovernor';

type StatsPanel = { dom: HTMLCanvasElement; update(value: number, maxValue: number): void };

export type VizStatsTier = FrameGovernorTier | 'paused';

export class VizStats {
  readonly dom: HTMLDivElement;
  private readonly panels: StatsPanel[] = [];
  private readonly fpsPanel: StatsPanel;
  private readonly msPanel: StatsPanel;
  private readonly memPanel: StatsPanel | null;
  private readonly idleDot: HTMLDivElement;
  private mode = 0;
  private beginTime = performance.now();
  // Seeded, not 0, or the first window folds all of page-load time in and reads ~0 FPS.
  private lastPresentTime = performance.now();
  private windowStart = performance.now();
  private windowFrames = 0;
  private windowElapsedMs = 0;
  private tier: VizStatsTier | null = null;

  constructor() {
    this.dom = document.createElement('div');
    this.dom.style.cssText = 'position:fixed;top:0;left:0;cursor:pointer;opacity:0.9;z-index:10000';
    this.dom.addEventListener('click', evt => {
      evt.preventDefault();
      this.showPanel((this.mode + 1) % this.panels.length);
    });

    this.fpsPanel = this.addPanel(new Stats.Panel('FPS', '#0ff', '#002'));
    this.msPanel = this.addPanel(new Stats.Panel('MS', '#0f0', '#020'));
    this.memPanel = (performance as any).memory ? this.addPanel(new Stats.Panel('MB', '#f08', '#201')) : null;

    this.idleDot = document.createElement('div');
    this.idleDot.style.cssText =
      'position:absolute;top:3px;right:3px;width:4px;height:4px;border-radius:50%;' +
      'background:#0ff;opacity:0;transition:opacity 120ms linear;pointer-events:none';
    this.dom.appendChild(this.idleDot);

    this.showPanel(0);
  }

  /** The dot is the only hint that a held reading is frozen rather than current. */
  setTier(tier: VizStatsTier) {
    if (tier === this.tier) {
      return;
    }
    this.tier = tier;
    this.idleDot.style.opacity = tier === 'render' ? '0' : '0.35';
  }

  begin() {
    this.beginTime = performance.now();
  }

  update() {
    const now = performance.now();
    this.msPanel.update(now - this.beginTime, 200);

    const gap = now - this.lastPresentTime;
    this.lastPresentTime = now;
    // Read live rather than latched, so a tier that flaps between changes still accumulates
    // its render frames. The gap check catches idle time the tier can't: the first frame of a
    // burst presents at 'render' tier but its gap spans the idle stretch before it — unbounded
    // after suspension, where no updates ran to reset the window.
    if (this.tier !== 'render' || gap > IDLE_GAP_MS) {
      this.windowFrames = 0;
      this.windowElapsedMs = 0;
      this.windowStart = now;
      return;
    }

    this.windowFrames += 1;
    this.windowElapsedMs += gap;
    if (
      now - this.windowStart >= FPS_WINDOW_MS &&
      this.windowFrames >= FPS_WINDOW_MIN_FRAMES &&
      this.windowElapsedMs > 0
    ) {
      this.fpsPanel.update((this.windowFrames * 1000) / this.windowElapsedMs, 100);
      const mem = (performance as any).memory;
      if (this.memPanel && mem) {
        this.memPanel.update(mem.usedJSHeapSize / 1048576, mem.jsHeapSizeLimit / 1048576);
      }
      this.windowStart = now;
      this.windowFrames = 0;
      this.windowElapsedMs = 0;
    }
  }

  private addPanel(panel: StatsPanel): StatsPanel {
    this.dom.appendChild(panel.dom);
    this.panels.push(panel);
    return panel;
  }

  private showPanel(id: number) {
    this.panels.forEach((p, i) => {
      p.dom.style.display = i === id ? 'block' : 'none';
    });
    this.mode = id;
  }
}
