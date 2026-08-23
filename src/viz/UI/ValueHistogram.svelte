<script lang="ts">
  import { histogramFromQuantiles, type ChannelStats } from 'src/geoscript/textureStats';
  import { redrawOn } from 'src/viz/UI/controlSection';

  let {
    stats,
    lo,
    hi,
    width,
    height,
    /** Stats indices to draw; defaults to all. Single channel draws gray, several overlay tinted. */
    channels,
  }: {
    stats: ChannelStats[];
    lo: number;
    hi: number;
    width: number;
    height: number;
    channels?: number[];
  } = $props();

  const TINTS = [
    'rgba(235, 80, 80, 0.6)',
    'rgba(80, 210, 80, 0.6)',
    'rgba(90, 130, 255, 0.6)',
    'rgba(200, 200, 200, 0.5)',
  ];

  const draw = (
    canvas: HTMLCanvasElement,
    p: { stats: ChannelStats[]; lo: number; hi: number; channels: number[] }
  ) => {
    const ctx = canvas.getContext('2d')!;
    const { width: w, height: h } = canvas;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#1a1a1a';
    ctx.fillRect(0, 0, w, h);
    // Unit-interval ticks under the bars, so the data's placement against [0, 1] reads at a
    // glance; inclusive so a window that ends exactly at 0 or 1 still shows them.
    ctx.fillStyle = '#0aa';
    for (const v of [0, 1]) {
      if (v >= p.lo && v <= p.hi)
        ctx.fillRect(Math.min(w - 1, Math.round(((v - p.lo) / (p.hi - p.lo)) * w)), 0, 1, h);
    }
    // 2px bars: spikes at the window edges (masks, clipped tails) stay visible.
    const bins = w >> 1;
    const hists = p.channels
      .filter(c => p.stats[c])
      .map(c => histogramFromQuantiles(p.stats[c].quantiles, p.lo, p.hi, bins));
    let peak = 1e-9;
    for (const hist of hists) for (const v of hist) peak = Math.max(peak, v);
    hists.forEach((hist, i) => {
      ctx.fillStyle = hists.length === 1 ? '#7a7a7a' : (TINTS[p.channels[i]] ?? TINTS[3]);
      for (let x = 0; x < bins; x++) {
        // sqrt scaling keeps sparse bins visible next to dominant peaks
        const bh = Math.sqrt(hist[x] / peak) * (h - 2);
        if (bh > 0) ctx.fillRect(x * 2, h - bh, 2, bh);
      }
    });
  };
  const histCanvas = redrawOn(draw);
</script>

<canvas
  {width}
  {height}
  style="width: {width}px; height: {height}px; display: block"
  use:histCanvas={{ stats, lo, hi, channels: channels ?? stats.map((_, i) => i) }}
></canvas>
