<script lang="ts">
  import { onMount } from 'svelte';
  import type * as Comlink from 'comlink';

  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';

  let {
    repl,
    ctxPtr,
    onclose,
  }: {
    repl: Comlink.Remote<GeoscriptWorkerMethods>;
    ctxPtr: number;
    onclose: () => void;
  } = $props();

  interface MeshEntry {
    ix: number;
    label: string;
    hasUvs: boolean;
    verts: Float32Array;
    indices: Uint32Array;
    uvs: Float32Array | null;
  }

  let meshes = $state.raw<MeshEntry[] | null>(null);
  let selectedIx = $state<number | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let stats = $state<{ islands: number; tris: number } | null>(null);

  const SIZE = 480;

  let fetchError = $state<string | null>(null);

  onMount(() => {
    (async () => {
      // One worker message: the fetch can't interleave with a queued eval's reset, which
      // would otherwise leave stale mesh indices panicking the wasm getters.
      const all = await repl.getAllRenderedMeshUvData(ctxPtr);
      meshes = all.map(({ verts, indices, uvs, sourceModule, material }, i) => {
        const src = sourceModule ? sourceModule.split(':').pop() : null;
        return {
          ix: i,
          label: `${i + 1}: ${src || 'root'}${material ? ` (${material})` : ''}`,
          hasUvs: !!uvs && uvs.length > 0,
          verts,
          indices,
          uvs,
        };
      });
      selectedIx = meshes.find(m => m.hasUvs)?.ix ?? null;
    })().catch(err => {
      fetchError = `failed to fetch mesh data: ${err}`;
    });
  });

  // Diverging blue→white→red for signed log2 area distortion in [-limit, limit].
  const distortionColor = (t: number): string => {
    const x = Math.max(-1, Math.min(1, t));
    const r = x > 0 ? 255 : Math.round(255 * (1 + x));
    const b = x < 0 ? 255 : Math.round(255 * (1 - x));
    const g = Math.round(255 * (1 - Math.abs(x)));
    return `rgb(${r},${g},${b})`;
  };

  const draw = (m: MeshEntry) => {
    if (!canvas || !m.uvs) return;
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, SIZE, SIZE);

    const { verts, indices, uvs } = m;
    const triCount = (indices.length / 3) | 0;

    // Per-triangle uv-area / surface-area ratio, normalized by the median so uniform
    // global scale reads as zero distortion.
    const ratios = new Float32Array(triCount);
    for (let t = 0; t < triCount; t += 1) {
      const [a, b, c] = [indices[t * 3], indices[t * 3 + 1], indices[t * 3 + 2]];
      const ux = uvs[b * 2] - uvs[a * 2];
      const uy = uvs[b * 2 + 1] - uvs[a * 2 + 1];
      const vx = uvs[c * 2] - uvs[a * 2];
      const vy = uvs[c * 2 + 1] - uvs[a * 2 + 1];
      const uvArea = Math.abs(ux * vy - uy * vx) * 0.5;
      const e1 = [
        verts[b * 3] - verts[a * 3],
        verts[b * 3 + 1] - verts[a * 3 + 1],
        verts[b * 3 + 2] - verts[a * 3 + 2],
      ];
      const e2 = [
        verts[c * 3] - verts[a * 3],
        verts[c * 3 + 1] - verts[a * 3 + 1],
        verts[c * 3 + 2] - verts[a * 3 + 2],
      ];
      const cx = e1[1] * e2[2] - e1[2] * e2[1];
      const cy = e1[2] * e2[0] - e1[0] * e2[2];
      const cz = e1[0] * e2[1] - e1[1] * e2[0];
      const surfArea = Math.sqrt(cx * cx + cy * cy + cz * cz) * 0.5;
      ratios[t] = surfArea > 1e-12 && uvArea > 1e-12 ? uvArea / surfArea : NaN;
    }
    const finite = Array.from(ratios).filter(Number.isFinite);
    finite.sort((a, b) => a - b);
    const median = finite[(finite.length / 2) | 0] || 1;

    // Fit the uv bbox (unioned with the unit square) into the canvas.
    let lo0 = 0;
    let lo1 = 0;
    let hi0 = 1;
    let hi1 = 1;
    for (let i = 0; i < uvs.length; i += 2) {
      lo0 = Math.min(lo0, uvs[i]);
      hi0 = Math.max(hi0, uvs[i]);
      lo1 = Math.min(lo1, uvs[i + 1]);
      hi1 = Math.max(hi1, uvs[i + 1]);
    }
    const pad = 8;
    const s = (SIZE - 2 * pad) / Math.max(hi0 - lo0, hi1 - lo1);
    const px = (u: number) => pad + (u - lo0) * s;
    // canvas y is down; uv v is up
    const py = (v: number) => SIZE - pad - (v - lo1) * s;

    const LIMIT = 2; // log2 half-range of the color scale (±4x areal distortion)
    for (let t = 0; t < triCount; t += 1) {
      const [a, b, c] = [indices[t * 3], indices[t * 3 + 1], indices[t * 3 + 2]];
      ctx.beginPath();
      ctx.moveTo(px(uvs[a * 2]), py(uvs[a * 2 + 1]));
      ctx.lineTo(px(uvs[b * 2]), py(uvs[b * 2 + 1]));
      ctx.lineTo(px(uvs[c * 2]), py(uvs[c * 2 + 1]));
      ctx.closePath();
      ctx.fillStyle = Number.isFinite(ratios[t])
        ? distortionColor(Math.log2(ratios[t] / median) / LIMIT)
        : '#666';
      ctx.fill();
      ctx.strokeStyle = 'rgba(0,0,0,0.55)';
      ctx.lineWidth = 0.4;
      ctx.stroke();
    }

    // unit-square outline: the tileable texture domain
    ctx.strokeStyle = '#0ff';
    ctx.lineWidth = 1;
    ctx.strokeRect(px(0), py(1), s, s);

    // Island count: connected components over shared uv-welded vertices.
    const parent = new Int32Array(verts.length / 3).map((_, i) => i);
    const find = (i: number): number => {
      while (parent[i] !== i) {
        parent[i] = parent[parent[i]];
        i = parent[i];
      }
      return i;
    };
    for (let t = 0; t < triCount; t += 1) {
      const ra = find(indices[t * 3]);
      const rb = find(indices[t * 3 + 1]);
      const rc = find(indices[t * 3 + 2]);
      parent[rb] = ra;
      parent[rc] = ra;
    }
    const roots = new Set<number>();
    for (let t = 0; t < triCount; t += 1) roots.add(find(indices[t * 3]));
    stats = { islands: roots.size, tris: triCount };
  };

  $effect(() => {
    if (meshes === null || selectedIx === null || !canvas) return;
    const m = meshes.find(m => m.ix === selectedIx);
    if (m) draw(m);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={e => e.target === e.currentTarget && onclose()}>
  <div class="dialog">
    <div class="head">
      <span>uv maps</span>
      <button class="close" onclick={onclose}>×</button>
    </div>
    {#if fetchError}
      <div class="msg">{fetchError}</div>
    {:else if meshes === null}
      <div class="msg">loading…</div>
    {:else if meshes.length === 0}
      <div class="msg">no rendered meshes</div>
    {:else}
      <div class="body">
        <div class="mesh-list">
          {#each meshes as m (m.ix)}
            <button
              class:selected={selectedIx === m.ix}
              disabled={!m.hasUvs}
              title={m.hasUvs ? m.label : `${m.label} — no uv attribute`}
              onclick={() => (selectedIx = m.ix)}
            >
              {m.label}
            </button>
          {/each}
        </div>
        <div class="view">
          {#if selectedIx !== null}
            <canvas bind:this={canvas} width={SIZE} height={SIZE}></canvas>
            <div class="legend">
              <span class="swatch" style="background: rgb(0,255,255)"></span>
              unit square
              <span class="swatch" style="background: rgb(255,120,120)"></span>
              stretched
              <span class="swatch" style="background: #fff"></span>
              uniform
              <span class="swatch" style="background: rgb(120,120,255)"></span>
              compressed
              {#if stats}
                <span class="stats">{stats.islands} islands · {stats.tris} tris</span>
              {/if}
            </div>
          {:else}
            <div class="msg">no mesh with uvs</div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dialog {
    background: #1d1d1d;
    border: 1px solid #444;
    box-shadow: 0 4px 18px rgba(0, 0, 0, 0.6);
    max-height: 90vh;
    display: flex;
    flex-direction: column;
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 4px 8px;
    background: #282828;
    font-size: 13px;
  }

  .close {
    background: none;
    border: none;
    color: #ccc;
    font-size: 16px;
    cursor: pointer;
    padding: 0 4px;
  }

  .body {
    display: flex;
    min-height: 0;
  }

  .mesh-list {
    display: flex;
    flex-direction: column;
    width: 170px;
    overflow-y: auto;
    border-right: 1px solid #333;
  }

  .mesh-list button {
    text-align: left;
    padding: 4px 6px;
    background: #2a2a2a;
    border: none;
    border-bottom: 1px solid #333;
    color: #ddd;
    cursor: pointer;
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mesh-list button.selected {
    background: #4a4a4a;
  }

  .mesh-list button:disabled {
    color: #666;
    cursor: default;
  }

  .view {
    padding: 8px;
  }

  canvas {
    background: #111;
    border: 1px solid #333;
  }

  .legend {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    color: #aaa;
    padding-top: 4px;
  }

  .swatch {
    display: inline-block;
    width: 9px;
    height: 9px;
    border: 1px solid #555;
  }

  .stats {
    margin-left: auto;
  }

  .msg {
    padding: 16px;
    font-size: 12px;
    color: #999;
  }
</style>
