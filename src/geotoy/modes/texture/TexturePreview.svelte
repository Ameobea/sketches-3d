<script lang="ts">
  import type { TextureChannel } from 'src/geoscript/geotoyAPIClient';
  import type { GeneratedTexture } from 'src/geoscript/runner/runner';
  import type { TextureMode } from 'src/geotoy/modes/texture/textureMode.svelte';

  let { mode, width, height }: { mode: TextureMode; width: number; height: number } = $props();

  const CHANNELS: TextureChannel[] = ['rgb', 'r', 'g', 'b'];
  const CHANNEL_IX: Record<TextureChannel, number> = { rgb: 0, r: 1, g: 2, b: 3 };
  const WRAP_IX = { repeat: 0, clamp: 1, mirror: 2 } as const;

  let canvas: HTMLCanvasElement | undefined = $state();
  let gl: WebGL2RenderingContext | null = null;
  let uniforms: Record<string, WebGLUniformLocation | null> = {};
  let floatLinear = false;
  let glTex: WebGLTexture | null = null;
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
out vec4 fragColor;

vec3 l2s(vec3 c) {
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

void main() {
  vec2 screen = vec2(gl_FragCoord.x, uCanvasSize.y - gl_FragCoord.y);
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

    vec4 t = texture(uTex, suv);
    c = uChannels == 1 ? vec3(t.r) : t.rgb;
    if (uChannelSel == 1) c = vec3(c.r);
    else if (uChannelSel == 2) c = vec3(c.g);
    else if (uChannelSel == 3) c = vec3(c.b);
    c = clamp(c, 0.0, 1.0);
    if (uSrgb) c = l2s(c);
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
    ]) {
      uniforms[name] = gl.getUniformLocation(prog, name);
    }
    gl.uniform1i(uniforms.uTex, 0);
  };

  const uploadTexture = (tex: GeneratedTexture) => {
    if (!gl || uploadedData === tex.data) return;
    if (!glTex) glTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, glTex);
    if (tex.channels === 1) {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R32F, tex.width, tex.height, 0, gl.RED, gl.FLOAT, tex.data);
    } else {
      const rgba = new Float32Array(tex.width * tex.height * 4);
      for (let i = 0; i < tex.width * tex.height; i++) {
        for (let ch = 0; ch < 3; ch++)
          rgba[i * 4 + ch] = tex.data[i * tex.channels + Math.min(ch, tex.channels - 1)];
        rgba[i * 4 + 3] = 1;
      }
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, tex.width, tex.height, 0, gl.RGBA, gl.FLOAT, rgba);
    }
    // mag NEAREST keeps texels crisp when zoomed in; min LINEAR needs the float-linear ext
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, floatLinear ? gl.LINEAR : gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    uploadedData = tex.data;
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
    gl.uniform2f(uniforms.uCanvasSize, pw, ph);
    gl.uniform2f(uniforms.uCenter, mode.center[0], mode.center[1]);
    gl.uniform2f(uniforms.uTilePx, mode.zoom * sel.width * dpr, mode.zoom * sel.height * dpr);
    gl.uniform1i(uniforms.uChannels, sel.channels);
    gl.uniform1i(uniforms.uChannelSel, CHANNEL_IX[mode.channel]);
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
      mode.center[1] + (e.clientY - height / 2) / (mode.zoom * sel.height),
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
      anchor[1] - (e.clientY - height / 2) / (zoom * sel.height),
    ];
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
      mode.center[1] - (e.clientY - dragLast[1]) / (mode.zoom * sel.height),
    ];
    dragLast = [e.clientX, e.clientY];
  };
  const onPointerUp = () => {
    dragLast = null;
  };
</script>

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
        {#each CHANNELS as ch (ch)}
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
    {:else}
      <span class="note">no visible outputs (solo active)</span>
    {/if}
  </div>
  {#if mode.selected}
    {@const sel = mode.selected}
    <div class="info panel">
      <span class="label">output</span>
      <span class="value accent">{sel.name}</span>
      <span class="label">size</span>
      <span class="value">{sel.width}×{sel.height}</span>
      <span class="label">format</span>
      <span class="value">{sel.channels === 1 ? 'r32f' : 'rgb32f'}</span>
      {#if sel.usage}
        <span class="label">usage</span>
        <span class="value">{sel.usage}</span>
      {/if}
      <span class="label">wrap</span>
      <span class="value">{sel.wrap}</span>
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

  .zoom {
    position: absolute;
    bottom: 6px;
    left: 6px;
    padding: 3px 7px;
    font-size: 10px;
    color: #999;
  }
</style>
