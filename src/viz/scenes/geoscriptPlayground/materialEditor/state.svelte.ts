import type { Texture } from 'src/geoscript/geotoyAPIClient';

class TextureStore {
  public textures = $state<Record<string, Texture>>({});
}

export const Textures = new TextureStore();