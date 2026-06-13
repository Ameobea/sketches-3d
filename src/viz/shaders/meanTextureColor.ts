import * as THREE from 'three';

const MAX_DIM = 128;

const SRGB_LUT = new Float32Array(256);
for (let i = 0; i < 256; i++) {
  const c = i / 255;
  SRGB_LUT[i] = c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

const readRgba = (img: any): { data: Uint8ClampedArray | Uint8Array; pixelCount: number } => {
  if (img.data && typeof img.width === 'number') {
    return { data: img.data, pixelCount: img.width * img.height };
  }

  const scale = Math.min(1, MAX_DIM / Math.max(img.width, img.height));
  const w = Math.max(1, Math.round(img.width * scale));
  const h = Math.max(1, Math.round(img.height * scale));
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!;
  ctx.drawImage(img, 0, 0, w, h);
  return { data: ctx.getImageData(0, 0, w, h).data, pixelCount: w * h };
};

/**
 * Mean color of a texture as the GPU would return it from its coarsest mip: linear for sRGB
 * textures, raw values otherwise, `(r, 0, 0, 1)` for red-format textures. Cached on
 * `tex.userData.meanColor`. Replaces per-fragment `texture(samp, uv, 99.)` mean fetches.
 */
export const getTextureMeanColor = (tex: THREE.Texture): THREE.Vector4 => {
  if (tex.userData.meanColor) {
    return tex.userData.meanColor;
  }

  const { data, pixelCount } = readRgba(tex.image);
  const srgb = tex.colorSpace === THREE.SRGBColorSpace;
  let r = 0,
    g = 0,
    b = 0,
    a = 0;
  for (let i = 0; i < pixelCount * 4; i += 4) {
    if (srgb) {
      r += SRGB_LUT[data[i]];
      g += SRGB_LUT[data[i + 1]];
      b += SRGB_LUT[data[i + 2]];
    } else {
      r += data[i] / 255;
      g += data[i + 1] / 255;
      b += data[i + 2] / 255;
    }
    a += data[i + 3] / 255;
  }
  const inv = 1 / pixelCount;
  const mean =
    tex.format === THREE.RedFormat || tex.format === THREE.RedIntegerFormat
      ? new THREE.Vector4(r * inv, 0, 0, 1)
      : new THREE.Vector4(r * inv, g * inv, b * inv, a * inv);
  tex.userData.meanColor = mean;
  return mean;
};
