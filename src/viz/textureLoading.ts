import * as THREE from 'three';
import { getEngine } from './engine';

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

export const loadTexture = (
  loader: THREE.ImageBitmapLoader,
  url: string,
  {
    mapping = THREE.UVMapping,
    wrapS = THREE.RepeatWrapping,
    wrapT = THREE.RepeatWrapping,
    magFilter = THREE.NearestFilter,
    minFilter = THREE.NearestMipmapLinearFilter,
    // minFilter = THREE.LinearFilter,
    format,
    type,
    anisotropy = 8,
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
    minFilter = THREE.NearestMipmapLinearFilter,
    format,
    type,
    anisotropy = 8,
  }: TextureArgs = {}
): Promise<THREE.Texture> => {
  const engine = await getEngine();

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

  const normalMapBytes: Uint8Array = engine.gen_normal_map_from_texture(
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
