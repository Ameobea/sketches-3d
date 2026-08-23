import type { TextureChannel } from 'src/geoscript/geotoyAPIClient';
import type { GeneratedTexture } from 'src/geoscript/runner/runner';

export interface PreviewCell {
  tex: GeneratedTexture;
  /** CSS px, canvas-relative, y down. */
  x: number;
  y: number;
  w: number;
  h: number;
  srgb: boolean;
  /** Value window shown as black→white (`[0, 1]` for images; the data's min–max when fitted). */
  range: [number, number];
}

export interface PreviewDraw {
  width: number;
  height: number;
  cells: PreviewCell[];
  /** UV at every cell's center. */
  center: [number, number];
  /** CSS px per UV unit, shared by every cell so they all show the same region. */
  tilePx: [number, number];
  channel: TextureChannel;
  tiled: boolean;
  stackT: number;
  /** The run's outputs; GPU copies of anything else are released. */
  live: readonly GeneratedTexture[];
}

export interface TexturePreviewGl {
  draw: (p: PreviewDraw) => void;
  dispose: () => void;
}

const CHANNEL_IX: Record<TextureChannel, number> = {
  rgb: 0,
  r: 1,
  g: 2,
  b: 3,
  a: 4,
};
const WRAP_IX = { repeat: 0, clamp: 1, mirror: 2 } as const;

const FRAG = `#version 300 es
precision highp float;
uniform vec2 uCellCenter;
uniform vec2 uCenter;
uniform vec2 uTilePx;
uniform int uChannels;
uniform int uChannelSel;
uniform bool uTiled;
uniform int uWrap;
uniform bool uSrgb;
uniform vec2 uRange;
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
  vec2 uv = uCenter + (screen - uCellCenter) / uTilePx;
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
    c = clamp((c - uRange.x) / (uRange.y - uRange.x), 0.0, 1.0);
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

const UNIFORMS = [
  'uCellCenter',
  'uCenter',
  'uTilePx',
  'uChannels',
  'uChannelSel',
  'uTiled',
  'uWrap',
  'uSrgb',
  'uRange',
  'uTex',
  'uTexArr',
  'uIsArray',
  'uStackT',
] as const;

interface Cached {
  tex: WebGLTexture;
  px: Float32Array;
  sig: string;
}

/** CPU-fallback 2x2 box-filter mip chain down to 1x1 (WebGL2 floor-size semantics; odd
 * dims clamp), for when GPU mipgen isn't available. */
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

export const createTexturePreviewGl = (canvas: HTMLCanvasElement): TexturePreviewGl | null => {
  const gl = canvas.getContext('webgl2', { antialias: false, depth: false });
  if (!gl) return null;
  // 16F is filterable in core WebGL2; mipgen additionally needs it color-renderable
  const colorRenderable = !!(
    gl.getExtension('EXT_color_buffer_float') || gl.getExtension('EXT_color_buffer_half_float')
  );
  const mipgenOk = new Map<number, boolean>();

  const compile = (type: number, src: string) => {
    const sh = gl.createShader(type)!;
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      console.error('texture preview shader:', gl.getShaderInfoLog(sh));
    }
    return sh;
  };
  const prog = gl.createProgram()!;
  gl.attachShader(prog, compile(gl.VERTEX_SHADER, VERT));
  gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FRAG));
  gl.linkProgram(prog);
  gl.useProgram(prog);
  const u = Object.fromEntries(UNIFORMS.map(n => [n, gl.getUniformLocation(prog, n)])) as Record<
    (typeof UNIFORMS)[number],
    WebGLUniformLocation | null
  >;
  gl.uniform1i(u.uTex, 0);
  gl.uniform1i(u.uTexArr, 1);
  gl.enable(gl.SCISSOR_TEST);

  /** Spec-valid with the render ext, but drivers may still refuse mipgen, and may refuse it for
   * only one target — an unnoticed refusal on TEXTURE_2D_ARRAY leaves the `texStorage3D` levels
   * undefined. Probe per target (getError is a blocking GPU-process round-trip, so once each)
   * and fall back to the CPU chain for whichever target refused. */
  const tryGpuMips = (target: number): boolean => {
    if (!colorRenderable || mipgenOk.get(target) === false) {
      return false;
    }
    gl.generateMipmap(target);
    if (!mipgenOk.has(target)) {
      mipgenOk.set(target, gl.getError() === gl.NO_ERROR);
    }
    return mipgenOk.get(target)!;
  };

  const cache = new Map<number, Cached>();
  let sweptFor: readonly GeneratedTexture[] | null = null;

  const upload = (t: GeneratedTexture): WebGLTexture => {
    const { width, height, layers, channels, textureId } = t;
    const px = channels === 3 ? t.rgba! : t.data!;
    const ch = channels === 3 ? 4 : channels;
    const sig = `${width}x${height}x${layers}x${ch}`;
    let c = cache.get(textureId);
    if (c && c.px === px) return c.tex;
    if (c && c.sig !== sig) {
      gl.deleteTexture(c.tex);
      c = undefined;
    }
    const fresh = !c;
    if (!c) {
      c = { tex: gl.createTexture()!, px, sig };
      cache.set(textureId, c);
    }
    c.px = px;
    // Half-float storage filled from the f32 pixels by the driver: display-only, so the lost
    // precision is invisible and VRAM halves. GL's (0, 0, 1) fill for missing g/b/a is what
    // the shader expects; 3ch uses the worker-expanded copy (see `GeneratedTexture.rgba`).
    const [ifmt, fmt] = ch === 1 ? [gl.R16F, gl.RED] : ch === 2 ? [gl.RG16F, gl.RG] : [gl.RGBA16F, gl.RGBA];
    const levels = 32 - Math.clz32(Math.max(width, height));
    const target = layers > 1 ? gl.TEXTURE_2D_ARRAY : gl.TEXTURE_2D;
    gl.activeTexture(layers > 1 ? gl.TEXTURE1 : gl.TEXTURE0);
    gl.bindTexture(target, c.tex);
    if (layers > 1) {
      if (fresh) gl.texStorage3D(target, levels, ifmt, width, height, layers);
      gl.texSubImage3D(target, 0, 0, 0, 0, width, height, layers, fmt, gl.FLOAT, px);
      if (!tryGpuMips(target)) {
        const layerSize = width * height * ch;
        for (let l = 0; l < layers; l++) {
          buildMips(px.subarray(l * layerSize, (l + 1) * layerSize), width, height, ch).forEach((m, i) => {
            gl.texSubImage3D(target, i + 1, 0, 0, l, m.w, m.h, 1, fmt, gl.FLOAT, m.data);
          });
        }
      }
    } else {
      gl.texImage2D(target, 0, ifmt, width, height, 0, fmt, gl.FLOAT, px);
      if (!tryGpuMips(target)) {
        buildMips(px, width, height, ch).forEach((m, i) => {
          gl.texImage2D(target, i + 1, ifmt, m.w, m.h, 0, fmt, gl.FLOAT, m.data);
        });
      }
    }
    if (fresh) {
      // mag NEAREST keeps texels crisp when zoomed in
      gl.texParameteri(target, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(target, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(target, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(target, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    }
    return c.tex;
  };

  const draw = ({ width, height, cells, center, tilePx, channel, tiled, stackT, live }: PreviewDraw) => {
    const dpr = window.devicePixelRatio || 1;
    const pw = Math.max(1, Math.round(width * dpr));
    const ph = Math.max(1, Math.round(height * dpr));
    if (canvas.width !== pw || canvas.height !== ph) {
      canvas.width = pw;
      canvas.height = ph;
    }
    // `live` is reassigned wholesale once per run, so identity is enough to skip the sweep on
    // every rAF of a pan or zoom drag.
    if (live !== sweptFor && cache.size) {
      const keep = new Set(live.map(t => t.textureId));
      for (const [id, c] of cache) {
        if (!keep.has(id)) {
          gl.deleteTexture(c.tex);
          cache.delete(id);
        }
      }
    }
    sweptFor = live;
    gl.viewport(0, 0, pw, ph);
    gl.scissor(0, 0, pw, ph);
    const bg = cells.length > 1 ? 0.16 : 0.05;
    gl.clearColor(bg, bg, bg, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(u.uCenter, center[0], center[1]);
    gl.uniform2f(u.uTilePx, tilePx[0] * dpr, tilePx[1] * dpr);
    gl.uniform1i(u.uTiled, tiled ? 1 : 0);
    gl.uniform1f(u.uStackT, stackT);
    for (const { tex, x, y, w, h, srgb, range } of cells) {
      const glTex = upload(tex);
      const x0 = Math.round(x * dpr);
      const x1 = Math.round((x + w) * dpr);
      const y0 = Math.round((height - y - h) * dpr);
      const y1 = Math.round((height - y) * dpr);
      gl.viewport(x0, y0, x1 - x0, y1 - y0);
      gl.scissor(x0, y0, x1 - x0, y1 - y0);
      gl.uniform2f(u.uCellCenter, (x0 + x1) / 2, (y0 + y1) / 2);
      const arr = tex.layers > 1;
      gl.activeTexture(arr ? gl.TEXTURE1 : gl.TEXTURE0);
      gl.bindTexture(arr ? gl.TEXTURE_2D_ARRAY : gl.TEXTURE_2D, glTex);
      gl.uniform1i(u.uIsArray, arr ? 1 : 0);
      gl.uniform1i(u.uChannels, tex.channels);
      // A stale 'a' selection on a non-RGBA output falls back to rgb instead of solid white
      gl.uniform1i(u.uChannelSel, CHANNEL_IX[channel === 'a' && tex.channels !== 4 ? 'rgb' : channel]);
      gl.uniform1i(u.uWrap, WRAP_IX[tex.wrap]);
      gl.uniform1i(u.uSrgb, srgb ? 1 : 0);
      gl.uniform2f(u.uRange, range[0], range[1]);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }
  };

  const dispose = () => {
    for (const c of cache.values()) gl.deleteTexture(c.tex);
    cache.clear();
    sweptFor = null;
    gl.deleteProgram(prog);
    gl.getExtension('WEBGL_lose_context')?.loseContext();
  };

  return { draw, dispose };
};
