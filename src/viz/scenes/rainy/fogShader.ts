import { BlendFunction, Effect, EffectAttribute } from 'postprocessing';
import type { WebGLRenderer, WebGLRenderTarget } from 'three';

import FogFragmentShader from './fogShader.frag?raw';

export class FogEffect extends Effect {
  constructor(blendFunction?: BlendFunction) {
    super('FogShader', FogFragmentShader, { attributes: EffectAttribute.DEPTH, blendFunction });
  }
}
