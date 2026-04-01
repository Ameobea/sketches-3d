import * as THREE from 'three';

import type { ObjectDef, ObjectGroupDef } from './types';

export const LEVEL_PLACEHOLDER_MAT = new THREE.MeshStandardMaterial({ color: 0x888888 });

type TransformDef = Pick<ObjectDef, 'position' | 'rotation' | 'scale'> | ObjectGroupDef;

export const applyTransform = (object: THREE.Object3D, def: TransformDef) => {
  const [px = 0, py = 0, pz = 0] = def.position ?? [];
  const [rx = 0, ry = 0, rz = 0] = def.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = def.scale ?? [];
  object.position.set(px, py, pz);
  object.rotation.set(rx, ry, rz, 'YXZ');
  object.scale.set(sx, sy, sz);
};

export const forEachMesh = (object: THREE.Object3D, cb: (mesh: THREE.Mesh) => void) => {
  object.traverse(child => {
    if (child instanceof THREE.Mesh) {
      cb(child);
    }
  });
};

export const applyShadowFlags = (
  object: THREE.Object3D,
  def: Pick<ObjectDef, 'castShadow' | 'receiveShadow'>
) => {
  const castShadow = def.castShadow ?? true;
  const receiveShadow = def.receiveShadow ?? true;
  forEachMesh(object, mesh => {
    mesh.castShadow = castShadow;
    mesh.receiveShadow = receiveShadow;
  });
};

export const assignMaterial = (object: THREE.Object3D, mat: THREE.Material) => {
  forEachMesh(object, mesh => {
    mesh.material = mat;
  });
};

interface InstantiateLevelObjectOpts {
  builtMaterials?: Map<string, THREE.Material>;
  fallbackMaterial?: THREE.Material;
  visible?: boolean;
}

export const instantiateLevelObject = (
  prototype: THREE.Object3D,
  def: ObjectDef,
  opts: InstantiateLevelObjectOpts = {}
) => {
  const clone = prototype.clone();

  applyTransform(clone, def);
  applyShadowFlags(clone, def);
  clone.userData = { ...clone.userData, ...(def.userData ?? {}), levelDefId: def.id };

  if (opts.visible !== undefined) {
    clone.visible = opts.visible;
  }

  if (def.material && opts.builtMaterials) {
    const mat = opts.builtMaterials.get(def.material) ?? opts.fallbackMaterial;
    if (mat) {
      assignMaterial(clone, mat);
    }
  }

  return clone;
};
