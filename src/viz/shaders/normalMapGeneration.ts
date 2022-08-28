import * as THREE from 'three';

export const generateNormalMapFromTexture = (
  engine: typeof import('../wasmComp/engine'),
  texture: THREE.Texture
): THREE.Texture => {
  const source = texture.image;
  if (!(source instanceof ImageBitmap)) {
    throw new Error('Expected texture to be an ImageBitmap');
  }

  const canvas = document.createElement('canvas');
  canvas.width = source.width;
  canvas.height = source.height;
  const ctx = canvas.getContext('2d')!;
  ctx.drawImage(source, 0, 0);
  const imageData = ctx.getImageData(0, 0, source.width, source.height);

  const normalMapBytes: Uint8Array = engine!.gen_normal_map_from_texture(
    new Uint8Array(imageData.data.buffer),
    source.height,
    source.width
  );
  const normalMapImageData = new ImageData(
    new Uint8ClampedArray(normalMapBytes.buffer),
    source.width,
    source.height
  );
  ctx.putImageData(normalMapImageData, 0, 0);
  const normalMapTexture = new THREE.CanvasTexture(
    canvas,
    THREE.UVMapping,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    THREE.NearestFilter,
    THREE.NearestFilter
  );
  normalMapTexture.needsUpdate = true;
  return normalMapTexture;
};
