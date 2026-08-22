<script lang="ts">
  import type { TextureChannel, TextureOutputGpuParams } from 'src/geoscript/geotoyAPIClient';
  import type { GeneratedTexture } from 'src/geoscript/runner/runner';
  import type { TextureMode } from 'src/geotoy/modes/texture/textureMode.svelte';
  import {
    createTexturePreviewGl,
    type PreviewCell,
    type PreviewDraw,
    type TexturePreviewGl,
  } from 'src/geotoy/modes/texture/texturePreviewGl';
  import {
    DEFAULT_FORMAT,
    defaultMagFilter,
    DEFAULT_MIN_FILTER,
    formatOptionsForChannels,
  } from 'src/geotoy/modules/proceduralTextures';
  import { setTopLeftSlot, topLeftOffset, TopLeftSlot } from 'src/geotoy/modules/topLeftOverlay.svelte';

  let {
    mode,
    width,
    height,
    onSetTextureParams,
  }: {
    mode: TextureMode;
    width: number;
    height: number;
    /** Persist a GPU-param edit for an output and rerun; empty-string fields clear. */
    onSetTextureParams?: (
      sourceModule: string,
      output: string,
      patch: Partial<TextureOutputGpuParams>
    ) => void;
  } = $props();

  const MIN_FILTERS = [
    'nearest',
    'linear',
    'nearest_mipmap_nearest',
    'nearest_mipmap_linear',
    'linear_mipmap_nearest',
    'linear_mipmap_linear',
  ];
  const MAG_FILTERS = ['nearest', 'linear'];
  const PARAM_DEFAULTS = {
    minFilter: DEFAULT_MIN_FILTER,
    magFilter: defaultMagFilter(),
    format: DEFAULT_FORMAT,
  };

  const setParam = (key: keyof TextureOutputGpuParams, value: string) => {
    const sel = mode.selected;
    if (!sel) return;
    // Selecting the default clears the stored override rather than pinning it.
    onSetTextureParams?.(sel.sourceModule, sel.name, { [key]: value === PARAM_DEFAULTS[key] ? '' : value });
  };

  const CHANNELS: TextureChannel[] = ['rgb', 'r', 'g', 'b', 'a'];

  let hudHeight = $state(0);
  $effect(() => setTopLeftSlot(TopLeftSlot.hud, hudHeight));

  let canvas: HTMLCanvasElement | undefined = $state();
  let glr = $state.raw<TexturePreviewGl | null>(null);
  $effect(() => {
    if (!canvas) return;
    glr = createTexturePreviewGl(canvas);
    return () => {
      glr?.dispose();
      glr = null;
    };
  });

  const GUTTER = 2;
  const shown: GeneratedTexture[] = $derived(
    mode.layout === 'grid' ? mode.visibleTextures : mode.selected ? [mode.selected] : []
  );
  /** Scale reference. Every cell shares one px-per-UV so the same region lines up across the
   *  grid; taking it from the first shown output rather than the selection is what keeps
   *  clicking a differently-sized cell from rescaling the whole grid. Single layout shows only
   *  the selected output, so there it is that output. */
  const ref: GeneratedTexture | null = $derived(shown[0] ?? null);
  /** A row up to two outputs, then `ceil(sqrt(n))` columns. Single mode is a one-cell grid,
   *  so the pointer math is the same in both. */
  const grid = $derived.by(() => {
    const n = shown.length;
    const cols = Math.max(1, n <= 2 ? n : Math.ceil(Math.sqrt(n)));
    const rows = Math.max(1, Math.ceil(n / cols));
    return { cols, rows, w: (width - (cols - 1) * GUTTER) / cols, h: (height - (rows - 1) * GUTTER) / rows };
  });
  const cells: PreviewCell[] = $derived(
    shown.map((tex, i) => ({
      tex,
      x: (i % grid.cols) * (grid.w + GUTTER),
      y: Math.floor(i / grid.cols) * (grid.h + GUTTER),
      w: grid.w,
      h: grid.h,
      srgb: mode.srgbFor(tex),
    }))
  );
  /** Cell under a point (CSS px); gutters and empty cells snap to the nearest. */
  const cellAt = (px: number, py: number) => {
    const { cols, rows, w, h } = grid;
    const c = Math.min(cols - 1, Math.max(0, Math.floor(px / (w + GUTTER))));
    const r = Math.min(rows - 1, Math.max(0, Math.floor(py / (h + GUTTER))));
    return { index: r * cols + c, cx: c * (w + GUTTER) + w / 2, cy: r * (h + GUTTER) + h / 2 };
  };

  /** The stack the `t` slider reads out against: the selected output if it is one, else the
   *  first shown stack. */
  const stackRef: GeneratedTexture | null = $derived(
    mode.selected && mode.selected.layers > 1 ? mode.selected : (shown.find(t => t.layers > 1) ?? null)
  );

  const fitView = () => {
    // A zero-area cell (editor panel dragged across the whole viewport) would otherwise poison
    // `zoom` with a non-positive value that the `zoom === null` refit guard can never clear.
    if (!ref || grid.w <= 0 || grid.h <= 0) {
      return;
    }
    mode.zoom = 0.8 * Math.min(grid.w / ref.width, grid.h / ref.height);
    mode.center = [0.5, 0.5];
  };

  let pending: PreviewDraw | null = null;
  let drawQueued = false;
  const scheduleDraw = () => {
    if (drawQueued) return;
    drawQueued = true;
    requestAnimationFrame(() => {
      drawQueued = false;
      if (glr && pending) glr.draw(pending);
    });
  };

  $effect(() => {
    if (!mode.center || mode.zoom === null) {
      fitView();
    }
    const { center, zoom } = mode;
    pending = {
      width,
      height,
      cells: ref && center && zoom !== null ? cells : [],
      center: center ?? [0.5, 0.5],
      tilePx: ref && zoom !== null ? [zoom * ref.width, zoom * ref.height] : [1, 1],
      channel: mode.channel,
      tiled: mode.tiled,
      stackT: mode.stackT,
      live: mode.textures,
    };
    if (glr) scheduleDraw();
  });

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    if (!ref || !mode.center || mode.zoom === null) {
      return;
    }
    const { cx, cy } = cellAt(e.clientX, e.clientY);
    const anchorU = mode.center[0] + (e.clientX - cx) / (mode.zoom * ref.width);
    const anchorV = mode.center[1] - (e.clientY - cy) / (mode.zoom * ref.height);
    const zoom = Math.min(512, Math.max(0.01, mode.zoom * Math.exp(-e.deltaY * 0.0015)));
    mode.zoom = zoom;
    mode.center = [
      anchorU - (e.clientX - cx) / (zoom * ref.width),
      anchorV + (e.clientY - cy) / (zoom * ref.height),
    ];
  };

  let shiftHeld = $state(false);

  const onStackTInput = (e: Event & { currentTarget: HTMLInputElement }) => {
    let v = e.currentTarget.valueAsNumber;
    // shift snaps to the nearest exact layer
    if (shiftHeld && stackRef) {
      v = Math.round(v * (stackRef.layers - 1)) / (stackRef.layers - 1);
    }
    mode.stackT = v;
  };

  let dragLast: [number, number] | null = null;
  let dragDist = 0;
  const onPointerDown = (e: PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragLast = [e.clientX, e.clientY];
    dragDist = 0;
  };
  const onPointerMove = (e: PointerEvent) => {
    if (!dragLast || !ref || !mode.center || mode.zoom === null) return;
    const dx = e.clientX - dragLast[0];
    const dy = e.clientY - dragLast[1];
    dragDist += Math.abs(dx) + Math.abs(dy);
    mode.center = [
      mode.center[0] - dx / (mode.zoom * ref.width),
      mode.center[1] + dy / (mode.zoom * ref.height),
    ];
    dragLast = [e.clientX, e.clientY];
  };
  const onPointerUp = (e: PointerEvent) => {
    // a click (not a drag) on a grid cell selects its output
    if (dragLast && dragDist < 4 && cells.length > 1) {
      const hit = cells[cellAt(e.clientX, e.clientY).index];
      if (hit) mode.selectedName = hit.tex.name;
    }
    dragLast = null;
  };
</script>

<svelte:window onkeydown={e => (shiftHeld = e.shiftKey)} onkeyup={e => (shiftHeld = e.shiftKey)} />

<canvas
  bind:this={canvas}
  style={`width: ${width}px; height: ${height}px;`}
  onwheel={onWheel}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  ondblclick={fitView}
></canvas>

<div class="hud" style={`width: ${width}px; height: ${height}px;`}>
  {#if cells.length > 1}
    {#each cells as cell (cell.tex.textureId)}
      <div
        class="cell"
        class:selected={cell.tex === mode.selected}
        style:left="{cell.x}px"
        style:top="{cell.y}px"
        style:width="{cell.w}px"
        style:height="{cell.h}px"
      >
        <span class="cell-label">{cell.tex.name}{cell.tex.usage ? ` · ${cell.tex.usage}` : ''}</span>
      </div>
    {/each}
  {/if}
  <div class="stack panel" bind:offsetHeight={hudHeight} style:top="{topLeftOffset(TopLeftSlot.hud)}px">
    {#if mode.selected}
      {@const sel = mode.selected}
      {#if mode.visibleTextures.length > 1}
        <div class="chips">
          {#each mode.visibleTextures as tex (tex.textureId)}
            <button
              class="chip"
              class:active={tex.name === sel.name}
              onclick={() => (mode.selectedName = tex.name)}
            >
              {tex.name}
            </button>
          {/each}
        </div>
      {/if}
      <div class="chips">
        {#each CHANNELS.filter(ch => ch !== 'a' || sel.channels === 4) as ch (ch)}
          <button class="chip" class:active={mode.channel === ch} onclick={() => (mode.channel = ch)}>
            {ch}
          </button>
        {/each}
        <span class="gap"></span>
        <button class="chip" class:active={mode.tiled} onclick={() => (mode.tiled = !mode.tiled)}>
          tile
        </button>
        <button class="chip" class:active={mode.srgb} onclick={() => (mode.srgbOverride = !mode.srgb)}>
          srgb
        </button>
        {#if mode.visibleTextures.length > 1}
          <button class="chip" class:active={mode.layout === 'grid'} onclick={mode.toggleLayout} title="G">
            grid
          </button>
        {/if}
      </div>
      {#if stackRef}
        <div class="stack-t" title="stack interpolation index; shift-drag snaps to layers">
          <span class="t-label">t</span>
          <input type="range" min="0" max="1" step="0.001" value={mode.stackT} oninput={onStackTInput} />
          <span class="t-readout">
            L{(mode.stackT * (stackRef.layers - 1)).toFixed(2)} / {stackRef.layers - 1}
          </span>
        </div>
      {/if}
    {:else}
      <span class="note">no visible outputs (solo active)</span>
    {/if}
  </div>
  {#if mode.selected}
    {@const sel = mode.selected}
    {@const fmt = sel.format ?? DEFAULT_FORMAT}
    <div class="info panel">
      <span class="label">output</span>
      <span class="value accent">{sel.name}</span>
      <span class="label">size</span>
      <span class="value">
        {sel.width}×{sel.height} · {sel.channels}ch f32{sel.layers > 1 ? ` · ${sel.layers} layers` : ''}
      </span>
      {#if sel.usage}
        <span class="label">usage</span>
        <span class="value">{sel.usage}</span>
      {/if}
      <span class="label">wrap</span>
      <span class="value">{sel.wrap}</span>
      <span class="label">format</span>
      <select class="value" value={fmt} onchange={e => setParam('format', e.currentTarget.value)}>
        {#each formatOptionsForChannels(sel.channels) as f (f)}
          <option value={f}>{f}</option>
        {/each}
      </select>
      <span class="label">min filter</span>
      <select
        class="value"
        value={sel.minFilter ?? DEFAULT_MIN_FILTER}
        onchange={e => setParam('minFilter', e.currentTarget.value)}
      >
        {#each MIN_FILTERS as f (f)}
          <option value={f} disabled={fmt.endsWith('32f') && f.includes('mipmap')}>{f}</option>
        {/each}
      </select>
      <span class="label">mag filter</span>
      <select
        class="value"
        value={sel.magFilter ?? defaultMagFilter()}
        onchange={e => setParam('magFilter', e.currentTarget.value)}
      >
        {#each MAG_FILTERS as f (f)}
          <option value={f}>{f}</option>
        {/each}
      </select>
      <span class="label">outputs</span>
      <span class="value">{mode.visibleTextures.length}</span>
    </div>
  {/if}
  {#if mode.selected && mode.zoom !== null}
    <span class="zoom panel">{Math.round(mode.zoom * 100)}%</span>
  {/if}
</div>

<style>
  canvas {
    position: fixed;
    top: 0;
    left: 0;
    background: #0d0d0d;
    touch-action: none;
  }

  .hud {
    position: fixed;
    top: 0;
    left: 0;
    pointer-events: none;
    user-select: none;
  }

  .panel {
    background: rgba(13, 13, 13, 0.9);
    border: 1px solid #2e2e2e;
  }

  .cell {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid transparent;
  }

  .cell.selected {
    border-color: #0aa;
  }

  .cell-label {
    position: absolute;
    top: 4px;
    right: 4px;
    padding: 3px 5px;
    font-size: 10px;
    line-height: 1;
    color: #ddd;
    background: rgba(13, 13, 13, 0.85);
    border: 1px solid #2e2e2e;
  }

  .stack {
    position: absolute;
    left: 6px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 5px;
  }

  .note {
    font-size: 11px;
    color: #999;
    line-height: 1;
    padding: 2px 3px;
  }

  .chips {
    display: flex;
    gap: 2px;
    pointer-events: auto;
  }

  .gap {
    width: 10px;
  }

  .stack-t {
    display: flex;
    align-items: center;
    gap: 6px;
    pointer-events: auto;
    font-size: 11px;
    color: #999;
    width: 100%;
  }

  .stack-t input[type='range'] {
    flex: 1;
    min-width: 120px;
    accent-color: #0aa;
  }

  .stack-t .t-readout {
    color: #ddd;
    min-width: 62px;
    text-align: right;
  }

  .chip {
    background: #141414;
    border: 1px solid #2e2e2e;
    color: #888;
    font-size: 13px;
    font-family: inherit;
    padding: 3px 8px 4px 8px;
    cursor: pointer;
    line-height: 1;
  }

  .chip:hover {
    background: #1e1e1e;
    color: #fff;
  }

  .chip.active {
    background: #242424;
    color: #fff;
  }

  .info {
    position: absolute;
    right: 6px;
    bottom: 6px;
    pointer-events: auto;
    display: grid;
    grid-template-columns: auto auto;
    gap: 3px 12px;
    padding: 7px 9px;
    font-size: 10px;
    line-height: 1;
  }

  .label {
    color: #777;
  }

  .value {
    color: #ddd;
  }

  .value.accent {
    color: #0ff;
  }

  select.value {
    appearance: none;
    background: #1c1c1c;
    border: 1px solid #3a3a3a;
    color: #ddd;
    font: inherit;
    padding: 1px 3px;
    margin: -2px 0;
    cursor: pointer;
    justify-self: start;
  }
  select.value:hover {
    border-color: #555;
  }

  .zoom {
    position: absolute;
    bottom: 6px;
    left: 6px;
    padding: 3px 7px;
    font-size: 10px;
    color: #999;
  }
</style>
