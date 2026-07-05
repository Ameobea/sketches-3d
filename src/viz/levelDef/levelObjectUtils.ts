import * as THREE from 'three';

import type { GeneratedObject } from 'src/geoscript/runner/types';
import { COMP_MATERIAL_PREFIX } from 'src/geoscript/runner/bakeComposition';
import type { ObjectDef, ObjectGroupDef } from './types';

export const LEVEL_PLACEHOLDER_MAT = new THREE.MeshStandardMaterial({ color: 0x888888 });

/** Level-material ids hidden from user-facing pickers: shared library refs and per-composition
 *  auto-imported materials. */
export const isInternalMaterialId = (id: string): boolean =>
  id.startsWith('__ASSETS__/') || id.startsWith(COMP_MATERIAL_PREFIX);

/** Material applied to selected objects in the level editor */
export const SELECTION_HIGHLIGHT_MAT = new THREE.MeshBasicMaterial({ color: 0x4488ff });

type TransformDef = Pick<ObjectDef, 'position' | 'rotation' | 'scale'> | ObjectGroupDef;

export const applyTransform = (object: THREE.Object3D, def: TransformDef) => {
  const [px = 0, py = 0, pz = 0] = def.position ?? [];
  const [rx = 0, ry = 0, rz = 0] = def.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = def.scale ?? [];
  object.position.set(px, py, pz);
  object.rotation.set(rx, ry, rz, 'YXZ');
  object.scale.set(sx, sy, sz);
};

/** Build a THREE.Mesh per `mesh`-type object in a geoscript run result, each with the given material. */
export const meshesFromRunObjects = (objects: GeneratedObject[], material: THREE.Material): THREE.Mesh[] => {
  const meshes: THREE.Mesh[] = [];
  for (const obj of objects) {
    if (obj.type !== 'mesh') {
      continue;
    }
    const mesh = new THREE.Mesh(obj.geometry, material);
    mesh.applyMatrix4(obj.transform);
    meshes.push(mesh);
  }
  return meshes;
};

/** The single mesh when there's exactly one; otherwise a Group wrapping all of them. */
export const groupOrSingle = (meshes: THREE.Mesh[]): THREE.Object3D => {
  if (meshes.length === 1) {
    return meshes[0];
  }
  const group = new THREE.Group();
  for (const mesh of meshes) {
    group.add(mesh);
  }
  return group;
};

export const forEachMesh = (object: THREE.Object3D, cb: (mesh: THREE.Mesh) => void) =>
  object.traverse(child => {
    if (child instanceof THREE.Mesh) {
      cb(child);
    }
  });

const applyShadowFlags = (object: THREE.Object3D, def: Pick<ObjectDef, 'castShadow' | 'receiveShadow'>) => {
  const castShadow = def.castShadow ?? true;
  const receiveShadow = def.receiveShadow ?? true;
  forEachMesh(object, mesh => {
    mesh.castShadow = castShadow;
    mesh.receiveShadow = receiveShadow;
  });
};

export const assignMaterial = (object: THREE.Object3D, mat: THREE.Material) =>
  forEachMesh(object, mesh => {
    mesh.material = mat;
  });

interface InstantiateLevelObjectOpts {
  builtMaterials?: Map<string, THREE.Material>;
  fallbackMaterial?: THREE.Material;
  visible?: boolean;
}

export const instantiateLevelObject = (
  prototype: THREE.Mesh,
  def: ObjectDef,
  opts: InstantiateLevelObjectOpts = {}
) => {
  const clone = prototype.clone();
  clone.name = def.id;

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
