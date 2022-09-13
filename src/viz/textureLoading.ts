import * as Comlink from 'comlink';
import * as THREE from 'three';
import { getEngine } from './engine';
import { clamp } from './util';

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

class WorkerPoolManager<T> {
  private idleWorkers: Comlink.Remote<T>[];

  /**
   * Populated when there are no idle workers.
   */
  private workQueue: [work: (worker: Comlink.Remote<T>) => any | Promise<any>, cb: (ret: any) => void][] = [];

  constructor(workers: Comlink.Remote<T>[]) {
    this.idleWorkers = workers;
  }

  public submitWork = async <R>(work: (worker: Comlink.Remote<T>) => R | Promise<R>): Promise<R> => {
    if (this.idleWorkers.length === 0) {
      return new Promise<R>(resolve => {
        this.workQueue.push([work, resolve]);
      });
    }

    const worker = this.idleWorkers.pop()!;

    let ret: { type: 'ok'; val: R } | { type: 'err'; err: any };

    try {
      const out = await work(worker);
      ret = { type: 'ok', val: out };
    } catch (err) {
      console.error('Error in worker', err);
      ret = { type: 'err', err };
    } finally {
      this.idleWorkers.push(worker);
    }

    if (this.workQueue.length > 0) {
      const [nextWork, cb] = this.workQueue.shift()!;
      this.submitWork(nextWork).then(cb);
    }

    if (ret.type === 'err') {
      throw ret.err;
    }
    return ret.val;
  };
}

let normalGenWorkers: Promise<WorkerPoolManager<any>> | null = null;

export const getNormalGenWorkers = async () => {
  if (normalGenWorkers) {
    return normalGenWorkers;
  }

  const wasmBytesAB = await fetch('/normal_map_gen.wasm').then(r => r.arrayBuffer());
  const wasmBytes = new Uint8Array(wasmBytesAB);

  normalGenWorkers = new Promise(async resolve => {
    const workerMod = await import('./normalGen.worker?worker');

    const numWorkers = clamp((navigator.hardwareConcurrency || 4) - 2, 1, 8);
    const workers = Array.from({ length: numWorkers }, () => {
      const worker = new workerMod.default();
      const wrapped = Comlink.wrap<any>(worker);
      wrapped.setWasmBytes(wasmBytes);
      return wrapped;
    });
    resolve(new WorkerPoolManager(workers));
  });
  return normalGenWorkers;
};

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

  // const packMode = packNormalGBA ? 1 : 0;
  // const normalMapBytes: Uint8Array = engine.gen_normal_map_from_texture(
  //   new Uint8Array(imageData.data.buffer),
  //   source.height,
  //   source.width,
  //   packMode
  // );
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
