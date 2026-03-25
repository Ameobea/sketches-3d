import * as THREE from 'three';

import { loadTexture } from 'src/viz/textureLoading';
import type { TextureDef } from './types';

const WRAPPING_MAP = {
  repeat: THREE.RepeatWrapping,
  clamp: THREE.ClampToEdgeWrapping,
  mirror: THREE.MirroredRepeatWrapping,
} as const;

const MAG_FILTER_MAP = {
  nearest: THREE.NearestFilter,
  linear: THREE.LinearFilter,
} as const;

const MIN_FILTER_MAP = {
  nearest: THREE.NearestFilter,
  nearestMipNearest: THREE.NearestMipMapNearestFilter,
  nearestMipLinear: THREE.NearestMipMapLinearFilter,
  linearMipLinear: THREE.LinearMipMapLinearFilter,
} as const;

const COLOR_SPACE_MAP = {
  srgb: THREE.SRGBColorSpace,
  '': THREE.NoColorSpace,
} as const;

const toTextureArgs = (def: TextureDef) => ({
  wrapS: WRAPPING_MAP[def.wrapS ?? 'repeat'],
  wrapT: WRAPPING_MAP[def.wrapT ?? 'repeat'],
  magFilter: MAG_FILTER_MAP[def.magFilter ?? 'nearest'],
  minFilter: MIN_FILTER_MAP[def.minFilter ?? 'nearestMipLinear'],
  anisotropy: def.anisotropy ?? 1,
  colorSpace: COLOR_SPACE_MAP[def.colorSpace ?? ''],
});

const loadWithRetry = async (
  loader: THREE.ImageBitmapLoader,
  def: TextureDef,
  maxRetries = 3
): Promise<THREE.Texture> => {
  let lastError: unknown;
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await loadTexture(loader, def.url, toTextureArgs(def));
    } catch (err) {
      lastError = err;
      if (attempt < maxRetries) {
        console.warn(`[levelDef] Texture fetch attempt ${attempt + 1} failed for "${def.url}", retrying...`);
      }
    }
  }
  throw lastError;
};

/**
 * Concurrency-limited texture loader. At most `concurrency` fetches run at a time.
 * Each fetch retries up to `maxRetries` times on failure.
 */
export class TextureFetchPool {
  private readonly loader = new THREE.ImageBitmapLoader();
  private active = 0;
  private readonly queue: Array<() => void> = [];

  constructor(
    private readonly concurrency = 8,
    private readonly maxRetries = 3
  ) {}

  load(def: TextureDef): Promise<THREE.Texture> {
    return new Promise<THREE.Texture>((resolve, reject) => {
      const task = () => {
        loadWithRetry(this.loader, def, this.maxRetries).then(
          tex => {
            this.active--;
            this.drain();
            resolve(tex);
          },
          err => {
            this.active--;
            this.drain();
            reject(err);
          }
        );
      };
      this.queue.push(task);
      this.drain();
    });
  }

  private drain() {
    while (this.active < this.concurrency && this.queue.length > 0) {
      this.active++;
      this.queue.shift()!();
    }
  }
}
