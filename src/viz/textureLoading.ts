import * as Comlink from 'comlink';
import * as THREE from 'three';

import { getNormalGenWorkers, getTextureCrossfadeWorkers } from './workerPool';

interface TextureArgs {
  mapping?: THREE.Mapping | undefined;
  wrapS?: THREE.Wrapping | undefined;
  wrapT?: THREE.Wrapping | undefined;
  magFilter?: THREE.TextureFilter | undefined;
  minFilter?: THREE.TextureFilter | undefined;
  format?: THREE.PixelFormat | undefined;
  type?: THREE.TextureDataType | undefined;
  anisotropy?: number | undefined;
  encoding?: THREE.TextureEncoding | undefined;
}

/**
 * Fetches and decodes the image at the given URL, decodes it into RGBA bytes, and returns it.
 */
export const loadRawTexture = async (url: string): Promise<ImageBitmap> => {
  const imgBitmap = await fetch(url)
    .then(r => r.blob())
    .then(b => createImageBitmap(b));
  return imgBitmap;
};

export const loadTexture = (
  loader: THREE.ImageBitmapLoader,
  url: string,
  {
    mapping = THREE.UVMapping,
    wrapS = THREE.RepeatWrapping,
    wrapT = THREE.RepeatWrapping,
    magFilter = THREE.NearestFilter,
    minFilter = THREE.NearestMipMapLinearFilter,
    // minFilter = THREE.LinearFilter,
    format,
    type,
    // anisotropy = 8,
    anisotropy = 1,
  }: TextureArgs = {}
) =>
  new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      url,
      imageBitmap => {
        const texture = new THREE.Texture(
          imageBitmap as any,
          mapping,
          wrapS,
          wrapT,
          magFilter,
          minFilter,
          format,
          type,
          anisotropy
        );
        texture.generateMipmaps = true;
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );

export const generateNormalMapFromTexture = async (
  texture: THREE.Texture,
  {
    mapping = THREE.UVMapping,
    wrapS = THREE.RepeatWrapping,
    wrapT = THREE.RepeatWrapping,
    magFilter = THREE.NearestFilter,
    minFilter = THREE.NearestMipMapLinearFilter,
    format = THREE.RGBAFormat,
    type = THREE.UnsignedByteType,
    anisotropy = 1,
  }: TextureArgs = {},
  packNormalGBA = false
): Promise<THREE.Texture> => {
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

  const workerPool = await getNormalGenWorkers();
  const normalMapBytes = await workerPool.submitWork(worker =>
    worker.genNormalMap(
      packNormalGBA,
      Comlink.transfer(new Uint8Array(imageData.data.buffer), [imageData.data.buffer]),
      imageData.height,
      imageData.width
    )
  );

  const normalMapImageData = new ImageData(
    new Uint8ClampedArray(normalMapBytes.buffer),
    source.width,
    source.height
  );
  ctx.putImageData(normalMapImageData, 0, 0);
  const normalMapTexture = new THREE.Texture(
    canvas,
    mapping,
    wrapS,
    wrapT,
    magFilter,
    minFilter,
    format,
    type,
    anisotropy
  );
  normalMapTexture.generateMipmaps = true;
  normalMapTexture.needsUpdate = true;
  return normalMapTexture;
};

export const genCrossfadedTexture = async (
  textures: ImageBitmap[],
  threshold: number,
  {
    mapping = THREE.UVMapping,
    wrapS = THREE.RepeatWrapping,
    wrapT = THREE.RepeatWrapping,
    magFilter = THREE.NearestFilter,
    minFilter = THREE.NearestMipMapLinearFilter,
    format = THREE.RGBAFormat,
    type = THREE.UnsignedByteType,
    anisotropy = 1,
  }: TextureArgs = {}
): Promise<THREE.Texture> => {
  const workersP = getTextureCrossfadeWorkers();
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d')!;

  const textureData = textures.map(source => {
    canvas.width = source.width;
    canvas.height = source.height;
    ctx.drawImage(source, 0, 0);
    const imageData = ctx.getImageData(0, 0, source.width, source.height);
    return new Uint8Array(imageData.data.buffer);
  });

  const workerPool = await workersP;
  const crossfadedTextureBytes: Uint8Array = await workerPool.submitWork(worker =>
    worker.genCrossfadedTexture(textureData, canvas.width, threshold)
  );
  if (
    crossfadedTextureBytes.length !==
    canvas.width * textures.length * canvas.height * textures.length * 4
  ) {
    throw new Error(
      `Unexpected length of crossfaded texture, expected ${
        canvas.width * textures.length * canvas.height * textures.length * 4
      }, got ${crossfadedTextureBytes.length}`
    );
  }
  const crossfadedTextureImageData = new ImageData(
    new Uint8ClampedArray(crossfadedTextureBytes.buffer),
    canvas.width * textures.length,
    canvas.height * textures.length
  );

  canvas.width = crossfadedTextureImageData.width;
  canvas.height = crossfadedTextureImageData.height;
  ctx.putImageData(crossfadedTextureImageData, 0, 0);
  const crossfadedTexture = new THREE.Texture(
    canvas,
    mapping,
    wrapS,
    wrapT,
    magFilter,
    minFilter,
    format,
    type,
    anisotropy
  );
  crossfadedTexture.generateMipmaps = true;
  crossfadedTexture.needsUpdate = true;
  return crossfadedTexture;
};
