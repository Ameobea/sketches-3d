import type { TextureDescriptor } from 'src/geoscript/geotoyAPIClient';

class TextureStore {
  public textures = $state<Record<string, TextureDescriptor>>({});
}

export const Textures = new TextureStore();
