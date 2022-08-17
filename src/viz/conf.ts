import * as THREE from 'three';

export const DefaultSceneName = 'bridge';

export const PlayerColliderHeight = 4.55;
export const PlayerColliderRadius = 0.35;

export const Locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
  bigCube: {
    pos: new THREE.Vector3(-281.43973660347024, 22.754156253511294, -8.752855510181472),
    rot: new THREE.Vector3(-0.504, 1.772, 0),
  },
};
