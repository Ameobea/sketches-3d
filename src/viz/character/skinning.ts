import * as THREE from 'three';

import type { CharacterSkeleton } from './skeleton';

const _ab = new THREE.Vector3();
const _ap = new THREE.Vector3();
const _p = new THREE.Vector3();

const segmentDistSq = (p: THREE.Vector3, a: THREE.Vector3, b: THREE.Vector3) => {
  _ab.subVectors(b, a);
  _ap.subVectors(p, a);
  const t = THREE.MathUtils.clamp(_ap.dot(_ab) / Math.max(_ab.lengthSq(), 1e-12), 0, 1);
  return _ap.addScaledVector(_ab, -t).lengthSq();
};

/**
 * Rigid skinning: every vertex follows the joint whose span is nearest.  Fits hard-surface
 * characters and needs no joint loops.  Writes `skinIndex`/`skinWeight` onto the geometry,
 * which must be in the skeleton's model space.
 */
export const skinRigid = (geometry: THREE.BufferGeometry, skeleton: CharacterSkeleton) => {
  const pos = geometry.attributes.position;
  const skinIndex = new Uint16Array(pos.count * 4);
  const skinWeight = new Float32Array(pos.count * 4);
  for (let i = 0; i < pos.count; i++) {
    _p.fromBufferAttribute(pos, i);
    let best = 0;
    let bestDist = Infinity;
    for (const seg of skeleton.segments) {
      const d = segmentDistSq(_p, seg.a, seg.b);
      if (d < bestDist) {
        bestDist = d;
        best = seg.boneIndex;
      }
    }
    skinIndex[i * 4] = best;
    skinWeight[i * 4] = 1;
  }
  geometry.setAttribute('skinIndex', new THREE.BufferAttribute(skinIndex, 4));
  geometry.setAttribute('skinWeight', new THREE.BufferAttribute(skinWeight, 4));
};
