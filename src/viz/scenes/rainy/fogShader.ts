import { BlendFunction, Effect, EffectAttribute } from 'postprocessing';

import FogFragmentShader from './fogShader.frag?raw';

export class FogEffect extends Effect {
  constructor(blendFunction?: BlendFunction) {
    super('FogShader', FogFragmentShader, { attributes: EffectAttribute.DEPTH, blendFunction });
  }
}
