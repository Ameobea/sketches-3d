import * as THREE from 'three';

export interface LegChain {
  thigh: THREE.Bone;
  shin: THREE.Bone;
  foot: THREE.Bone;
}

export interface SkinSegment {
  boneIndex: number;
  a: THREE.Vector3;
  b: THREE.Vector3;
}

export interface CharacterSkeleton {
  root: THREE.Bone;
  /** Every joint; the index is the skin index. */
  bones: THREE.Bone[];
  /** Rest-pose joint positions in model space, parallel to `bones`. */
  restPositions: THREE.Vector3[];
  /** Parent→child spans; vertices near a span skin to its parent joint. */
  segments: SkinSegment[];
  legs: LegChain[];
  /** Lowest foot at rest, or the lowest joint when there are no legs. */
  floorY: number;
}

const JoinTolerance = 1e-3;

/**
 * Bone tree from rendered polylines.  Consecutive points are one bone each; chain 0 is the
 * root chain; a later chain attaches where its first point coincides with an existing joint.
 * Every later chain with at least two segments is a leg whose last two bones are the IK pair.
 * Joints carry position only (identity rest rotation), so rest directions come from positions.
 */
export const buildSkeleton = (chains: Float32Array[]): CharacterSkeleton => {
  if (chains.length === 0) {
    throw new Error('character has no bone chains');
  }
  const bones: THREE.Bone[] = [];
  const restPositions: THREE.Vector3[] = [];
  const segments: SkinSegment[] = [];
  const legs: LegChain[] = [];

  const addJoint = (pos: THREE.Vector3, parent: THREE.Bone | null, name: string) => {
    const bone = new THREE.Bone();
    bone.name = name;
    if (parent) {
      const parentIx = bones.indexOf(parent);
      bone.position.subVectors(pos, restPositions[parentIx]);
      parent.add(bone);
      segments.push({ boneIndex: parentIx, a: restPositions[parentIx].clone(), b: pos.clone() });
    } else {
      bone.position.copy(pos);
    }
    bones.push(bone);
    restPositions.push(pos.clone());
    return bone;
  };

  chains.forEach((flat, ci) => {
    const pts: THREE.Vector3[] = [];
    for (let i = 0; i + 2 < flat.length; i += 3) {
      pts.push(new THREE.Vector3(flat[i], flat[i + 1], flat[i + 2]));
    }
    if (pts.length < 2) {
      throw new Error(`bone chain ${ci} needs at least two points`);
    }
    let parent: THREE.Bone | null = null;
    let start = 0;
    if (ci > 0) {
      const ix = restPositions.findIndex(p => p.distanceTo(pts[0]) < JoinTolerance);
      if (ix < 0) {
        throw new Error(`bone chain ${ci} does not start on an existing joint`);
      }
      parent = bones[ix];
      start = 1;
    }
    const chainBones: THREE.Bone[] = parent ? [parent] : [];
    for (let k = start; k < pts.length; k++) {
      parent = addJoint(pts[k], parent, `chain${ci}_${k}`);
      chainBones.push(parent);
    }
    if (ci > 0 && chainBones.length >= 3) {
      const n = chainBones.length;
      legs.push({ thigh: chainBones[n - 3], shin: chainBones[n - 2], foot: chainBones[n - 1] });
    }
  });

  const restOf = (bone: THREE.Bone) => restPositions[bones.indexOf(bone)];
  const floorY = legs.length
    ? Math.min(...legs.map(l => restOf(l.foot).y))
    : Math.min(...restPositions.map(p => p.y));
  return { root: bones[0], bones, restPositions, segments, legs, floorY };
};

export const restPosition = (skeleton: CharacterSkeleton, bone: THREE.Bone) =>
  skeleton.restPositions[skeleton.bones.indexOf(bone)];
