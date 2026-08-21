<script lang="ts">
  import type { TextureChannel, TextureOutputGpuParams } from 'src/geoscript/geotoyAPIClient';
  import type { GeneratedTexture } from 'src/geoscript/runner/runner';
  import type { TextureMode } from 'src/geotoy/modes/texture/textureMode.svelte';
  import {
    DEFAULT_FORMAT,
    DEFAULT_MAG_FILTER,
    DEFAULT_MIN_FILTER,
    formatOptionsForChannels,
  } from 'src/geotoy/modules/proceduralTextures';

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
    magFilter: DEFAULT_MAG_FILTER,
    format: DEFAULT_FORMAT,
  };

  const setParam = (key: keyof TextureOutputGpuParams, value: string) => {
    const sel = mode.selected;
    if (!sel) return;
    // Selecting the default clears the stored override rather than pinning it.
    onSetTextureParams?.(sel.sourceModule, sel.name, { [key]: value === PARAM_DEFAULTS[key] ? '' : value });
  };

  const CHANNELS: TextureChannel[] = ['rgb', 'r', 'g', 'b', 'a'];
  const CHANNEL_IX: Record<TextureChannel, number> = { rgb: 0, r: 1, g: 2, b: 3, a: 4 };
  const WRAP_IX = { repeat: 0, clamp: 1, mirror: 2 } as const;

  let canvas: HTMLCanvasElement | undefined = $state();
  let gl: WebGL2RenderingContext | null = null;
  let uniforms: Record<string, WebGLUniformLocation | null> = {};
  let floatLinear = false;
  let gpuMips = false;
  let gpuMipsVerified = false;
  let glTex: WebGLTexture | null = null;
  let glTexArr: WebGLTexture | null = null;
  let glTexArrSig: string | null = null;
  let uploadedData: Float32Array | null = null;

  const dpr = window.devicePixelRatio || 1;

  const FRAG = `#version 300 es
precision highp float;
uniform vec2 uCanvasSize;
uniform vec2 uCenter;
uniform vec2 uTilePx;
uniform int uChannels;
uniform int uChannelSel;
uniform bool uTiled;
uniform int uWrap;
uniform bool uSrgb;
uniform sampler2D uTex;
uniform highp sampler2DArray uTexArr;
uniform bool uIsArray;
uniform float uStackT;
out vec4 fragColor;

vec4 sampleSelected(vec2 suv) {
  // Analytic UV gradients (uv is linear in screen space): keeps mip selection correct
  // and seam-free across the fract() tile boundary in tiled mode.
  vec2 gx = vec2(1.0 / uTilePx.x, 0.0);
  vec2 gy = vec2(0.0, 1.0 / uTilePx.y);
  if (uIsArray) {
    float layers = float(textureSize(uTexArr, 0).z);
    float layer = clamp(uStackT, 0.0, 1.0) * (layers - 1.0);
    float lo = floor(layer);
    return mix(
      textureGrad(uTexArr, vec3(suv, lo), gx, gy),
      textureGrad(uTexArr, vec3(suv, min(lo + 1.0, layers - 1.0)), gx, gy),
      fract(layer)
    );
  }
  return textureGrad(uTex, suv, gx, gy);
}

vec3 l2s(vec3 c) {
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

void main() {
  // y-up: row 0 / uv.y = 0 at the bottom, matching GL texture space and mesh UVs.
  vec2 screen = gl_FragCoord.xy;
  vec2 uv = uCenter + (screen - uCanvasSize * 0.5) / uTilePx;
  vec2 d = abs(uv - 0.5);

  float ext = uTiled ? 1.5 : 0.5;
  vec3 c;
  if (d.x > ext || d.y > ext) {
    vec2 cell = floor(screen / 12.0);
    float ck = mod(cell.x + cell.y, 2.0);
    c = vec3(0.05 + 0.012 * ck);
  } else {
    vec2 suv = uv;
    if (uWrap == 0) suv = fract(uv);
    else if (uWrap == 1) suv = clamp(uv, 0.0, 1.0);
    else { suv = mod(uv, 2.0); suv = 1.0 - abs(suv - 1.0); }

    vec4 t = sampleSelected(suv);
    c = uChannels == 1 ? vec3(t.r) : t.rgb;
    if (uChannelSel == 1) c = vec3(c.r);
    else if (uChannelSel == 2) c = vec3(c.g);
    else if (uChannelSel == 3) c = vec3(c.b);
    else if (uChannelSel == 4) c = vec3(uChannels == 4 ? t.a : 1.0);
    c = clamp(c, 0.0, 1.0);
    if (uSrgb) c = l2s(c);
    if (uChannels == 4 && uChannelSel == 0) {
      // straight-alpha composite over a checkerboard so transparency reads visually
      vec2 acell = floor(screen / 12.0);
      vec3 backdrop = vec3(0.25 + 0.1 * mod(acell.x + acell.y, 2.0));
      c = mix(backdrop, c, clamp(t.a, 0.0, 1.0));
    }
  }

  // Unit-tile frame, coverage-blended (max-norm signed distance in device px) so
  // subpixel pan moves it smoothly instead of popping per side.
  vec2 bpx = (d - 0.5) * uTilePx;
  float bd = abs(max(bpx.x, bpx.y));
  c = mix(c, vec3(0.2), 1.0 - smoothstep(0.5, 1.5, bd));
  fragColor = vec4(c, 1.0);
}`;

  const VERT = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

  const initGL = (c: HTMLCanvasElement) => {
    gl = c.getContext('webgl2', { antialias: false, depth: false });
    if (!gl) return;
    floatLinear = !!gl.getExtension('OES_texture_float_linear');
    // generateMipmap on 32F needs the format renderable (float-render ext) AND filterable
    gpuMips = floatLinear && !!gl.getExtension('EXT_color_buffer_float');
    const compile = (type: number, src: string) => {
      const sh = gl!.createShader(type)!;
      gl!.shaderSource(sh, src);
      gl!.compileShader(sh);
      if (!gl!.getShaderParameter(sh, gl!.COMPILE_STATUS)) {
        console.error('texture preview shader:', gl!.getShaderInfoLog(sh));
      }
      return sh;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    gl.useProgram(prog);
    for (const name of [
      'uCanvasSize',
      'uCenter',
      'uTilePx',
      'uChannels',
      'uChannelSel',
      'uTiled',
      'uWrap',
      'uSrgb',
      'uTex',
      'uTexArr',
      'uIsArray',
      'uStackT',
    ]) {
      uniforms[name] = gl.getUniformLocation(prog, name);
    }
    gl.uniform1i(uniforms.uTex, 0);
    gl.uniform1i(uniforms.uTexArr, 1);
  };

  /** Spec-valid with both float exts, but drivers may still refuse 32F mipgen; probe the
   * first call (getError is a blocking GPU-process round-trip, so skip once verified) and
   * latch off to the CPU chain on refusal. */
  const tryGpuMips = (target: number): boolean => {
    if (!gpuMips) return false;
    gl!.generateMipmap(target);
    if (!gpuMipsVerified) {
      if (gl!.getError() !== gl!.NO_ERROR) {
        gpuMips = false;
        return false;
      }
      gpuMipsVerified = true;
    }
    return true;
  };

  /** CPU-fallback 2x2 box-filter mip chain down to 1x1 (WebGL2 floor-size semantics; odd
   * dims clamp), for when GPU mipgen on 32F targets isn't available. */
  const buildMips = (
    data: Float32Array,
    w: number,
    h: number,
    ch: number
  ): { data: Float32Array; w: number; h: number }[] => {
    const levels: { data: Float32Array; w: number; h: number }[] = [];
    let src = data;
    let sw = w;
    let sh = h;
    while (sw > 1 || sh > 1) {
      const dw = Math.max(1, sw >> 1);
      const dh = Math.max(1, sh >> 1);
      const dst = new Float32Array(dw * dh * ch);
      for (let y = 0; y < dh; y++) {
        const y0 = Math.min(y * 2, sh - 1) * sw;
        const y1 = Math.min(y * 2 + 1, sh - 1) * sw;
        for (let x = 0; x < dw; x++) {
          const x0 = Math.min(x * 2, sw - 1);
          const x1 = Math.min(x * 2 + 1, sw - 1);
          for (let c = 0; c < ch; c++) {
            dst[(y * dw + x) * ch + c] =
              0.25 *
              (src[(y0 + x0) * ch + c] +
                src[(y0 + x1) * ch + c] +
                src[(y1 + x0) * ch + c] +
                src[(y1 + x1) * ch + c]);
          }
        }
      }
      levels.push({ data: dst, w: dw, h: dh });
      src = dst;
      sw = dw;
      sh = dh;
    }
    return levels;
  };

  const setSamplerParams = (target: number, minFilter: number) => {
    gl!.texParameteri(target, gl!.TEXTURE_MAG_FILTER, gl!.NEAREST);
    gl!.texParameteri(target, gl!.TEXTURE_MIN_FILTER, minFilter);
    gl!.texParameteri(target, gl!.TEXTURE_WRAP_S, gl!.CLAMP_TO_EDGE);
    gl!.texParameteri(target, gl!.TEXTURE_WRAP_T, gl!.CLAMP_TO_EDGE);
  };

  const uploadTexture = (tex: GeneratedTexture) => {
    const { width, height, layers, channels, data, rgba } = tex;
    const px = channels === 3 ? rgba! : data!;
    if (!gl || uploadedData === px) return;
    // mag NEAREST keeps texels crisp when zoomed in; min uses the mip chain (trilinear
    // when the float-linear ext allows filtering, per-level nearest otherwise)
    const minFilter = floatLinear ? gl.LINEAR_MIPMAP_LINEAR : gl.NEAREST_MIPMAP_NEAREST;
    const levels = 32 - Math.clz32(Math.max(width, height));
    // Raw pixels upload direct in a channel-matched format — GL's (0, 0, 1) fill for
    // missing g/b/a matches the display shader's expectations. 3ch is the exception and
    // uses the worker-expanded copy (see `GeneratedTexture.rgba`).
    const ch = channels === 3 ? 4 : channels;
    const [ifmt, fmt] = ch === 1 ? [gl.R32F, gl.RED] : ch === 2 ? [gl.RG32F, gl.RG] : [gl.RGBA32F, gl.RGBA];
    if (layers > 1) {
      gl.activeTexture(gl.TEXTURE1);
      // immutable storage can't be resized, so recreate on dimension/format change
      const sig = `${width}x${height}x${layers}x${ch}`;
      if (glTexArr && glTexArrSig !== sig) {
        gl.deleteTexture(glTexArr);
        glTexArr = null;
      }
      if (!glTexArr) {
        glTexArr = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D_ARRAY, glTexArr);
        gl.texStorage3D(gl.TEXTURE_2D_ARRAY, levels, ifmt, width, height, layers);
        glTexArrSig = sig;
      } else {
        gl.bindTexture(gl.TEXTURE_2D_ARRAY, glTexArr);
      }
      gl.texSubImage3D(gl.TEXTURE_2D_ARRAY, 0, 0, 0, 0, width, height, layers, fmt, gl.FLOAT, px);
      if (!tryGpuMips(gl.TEXTURE_2D_ARRAY)) {
        const layerSize = width * height * ch;
        for (let l = 0; l < layers; l++) {
          const mips = buildMips(px.subarray(l * layerSize, (l + 1) * layerSize), width, height, ch);
          for (let i = 0; i < mips.length; i++) {
            const m = mips[i];
            gl.texSubImage3D(gl.TEXTURE_2D_ARRAY, i + 1, 0, 0, l, m.w, m.h, 1, fmt, gl.FLOAT, m.data);
          }
        }
      }
      setSamplerParams(gl.TEXTURE_2D_ARRAY, minFilter);
      gl.activeTexture(gl.TEXTURE0);
    } else {
      if (!glTex) glTex = gl.createTexture();
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, glTex);
      gl.texImage2D(gl.TEXTURE_2D, 0, ifmt, width, height, 0, fmt, gl.FLOAT, px);
      if (!tryGpuMips(gl.TEXTURE_2D)) {
        const mips = buildMips(px, width, height, ch);
        mips.forEach(({ data, w, h }, i) => {
          gl!.texImage2D(gl!.TEXTURE_2D, i + 1, ifmt, w, h, 0, fmt, gl!.FLOAT, data);
        });
      }
      setSamplerParams(gl.TEXTURE_2D, minFilter);
    }
    uploadedData = px;
  };

  const fitView = (tex: GeneratedTexture) => {
    mode.zoom = 0.8 * Math.min(width / tex.width, height / tex.height);
    mode.center = [0.5, 0.5];
  };

  const draw = () => {
    const c = canvas;
    if (!c || !gl) return;
    const pw = Math.max(1, Math.round(width * dpr));
    const ph = Math.max(1, Math.round(height * dpr));
    if (c.width !== pw || c.height !== ph) {
      c.width = pw;
      c.height = ph;
    }
    gl.viewport(0, 0, pw, ph);

    const sel = mode.selected;
    if (!sel || !mode.center || mode.zoom === null) {
      gl.clearColor(0.05, 0.05, 0.05, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      return;
    }
    uploadTexture(sel);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, glTex);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D_ARRAY, glTexArr);
    gl.uniform1i(uniforms.uIsArray, sel.layers > 1 ? 1 : 0);
    gl.uniform1f(uniforms.uStackT, mode.stackT);
    gl.uniform2f(uniforms.uCanvasSize, pw, ph);
    gl.uniform2f(uniforms.uCenter, mode.center[0], mode.center[1]);
    gl.uniform2f(uniforms.uTilePx, mode.zoom * sel.width * dpr, mode.zoom * sel.height * dpr);
    gl.uniform1i(uniforms.uChannels, sel.channels);
    // A stale 'a' selection on a non-RGBA output falls back to rgb instead of solid white
    const channel = mode.channel === 'a' && sel.channels !== 4 ? 'rgb' : mode.channel;
    gl.uniform1i(uniforms.uChannelSel, CHANNEL_IX[channel]);
    gl.uniform1i(uniforms.uTiled, mode.tiled ? 1 : 0);
    gl.uniform1i(uniforms.uWrap, WRAP_IX[sel.wrap]);
    gl.uniform1i(uniforms.uSrgb, mode.srgb ? 1 : 0);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  let drawQueued = false;
  const scheduleDraw = () => {
    if (drawQueued) return;
    drawQueued = true;
    requestAnimationFrame(() => {
      drawQueued = false;
      draw();
    });
  };

  $effect(() => {
    if (canvas && !gl) initGL(canvas);
    void mode.selected;
    void mode.channel;
    void mode.tiled;
    void mode.srgb;
    void mode.center;
    void mode.zoom;
    void mode.stackT;
    void width;
    void height;
    if (mode.selected && (!mode.center || mode.zoom === null)) fitView(mode.selected);
    scheduleDraw();
  });

  const uvAt = (e: PointerEvent | WheelEvent): [number, number] | null => {
    const sel = mode.selected;
    if (!sel || !mode.center || mode.zoom === null) return null;
    return [
      mode.center[0] + (e.clientX - width / 2) / (mode.zoom * sel.width),
      mode.center[1] - (e.clientY - height / 2) / (mode.zoom * sel.height),
    ];
  };

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    const sel = mode.selected;
    const anchor = uvAt(e);
    if (!sel || !anchor || mode.zoom === null) return;
    const zoom = Math.min(512, Math.max(0.01, mode.zoom * Math.exp(-e.deltaY * 0.0015)));
    mode.zoom = zoom;
    mode.center = [
      anchor[0] - (e.clientX - width / 2) / (zoom * sel.width),
      anchor[1] + (e.clientY - height / 2) / (zoom * sel.height),
    ];
  };

  let shiftHeld = $state(false);

  const onStackTInput = (e: Event & { currentTarget: HTMLInputElement }) => {
    const sel = mode.selected;
    let v = e.currentTarget.valueAsNumber;
    // shift snaps to the nearest exact layer
    if (shiftHeld && sel && sel.layers > 1) {
      v = Math.round(v * (sel.layers - 1)) / (sel.layers - 1);
    }
    mode.stackT = v;
  };

  let dragLast: [number, number] | null = null;
  const onPointerDown = (e: PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragLast = [e.clientX, e.clientY];
  };
  const onPointerMove = (e: PointerEvent) => {
    const sel = mode.selected;
    if (!dragLast || !sel || !mode.center || mode.zoom === null) return;
    mode.center = [
      mode.center[0] - (e.clientX - dragLast[0]) / (mode.zoom * sel.width),
      mode.center[1] + (e.clientY - dragLast[1]) / (mode.zoom * sel.height),
    ];
    dragLast = [e.clientX, e.clientY];
  };
  const onPointerUp = () => {
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
  ondblclick={() => mode.selected && fitView(mode.selected)}
></canvas>

<div class="hud" style={`width: ${width}px; height: ${height}px;`}>
  <div class="stack panel">
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
      </div>
      {#if sel.layers > 1}
        <div class="stack-t" title="stack interpolation index; shift-drag snaps to layers">
          <span class="t-label">t</span>
          <input type="range" min="0" max="1" step="0.001" value={mode.stackT} oninput={onStackTInput} />
          <span class="t-readout">
            L{(mode.stackT * (sel.layers - 1)).toFixed(2)} / {sel.layers - 1}
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
        value={sel.magFilter ?? DEFAULT_MAG_FILTER}
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

  .stack {
    position: absolute;
    top: 6px;
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
