import type { Texture } from './textureStore';

class TextureStore {
  public textures = $state<Record<string, Texture>>({});
}

export const Textures = new TextureStore();
