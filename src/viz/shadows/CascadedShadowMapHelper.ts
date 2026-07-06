import * as THREE from 'three';

import type { CascadedShadowMap } from './CascadedShadowMap';

const CASCADE_COLORS = [0xff4444, 0x44ff44, 0x4488ff, 0xffdd44];

/** Draws one colored `CameraHelper` per cascade so the fitted ortho shadow bounds are visible. */
export class CascadedShadowMapHelper extends THREE.Group {
  private readonly helpers: THREE.CameraHelper[] = [];

  constructor(csm: CascadedShadowMap) {
    super();
    csm.cameras.forEach((cam, i) => {
      const helper = new THREE.CameraHelper(cam);
      const color = new THREE.Color(CASCADE_COLORS[i % CASCADE_COLORS.length]);
      helper.setColors(color, color, color, color, color);
      this.helpers.push(helper);
      this.add(helper);
    });
  }

  update() {
    for (const helper of this.helpers) {
      helper.update();
    }
  }

  dispose() {
    for (const helper of this.helpers) {
      helper.dispose();
    }
  }
}
