import type * as THREE from 'three';

/** A level's character asset as the loader resolves it: meshes in one shared space plus bone chains. */
export interface CharacterAssetResult {
  meshes: { mesh: THREE.Mesh; materialName?: string }[];
  /** Flat xyz polylines in the meshes' space; see `buildSkeleton` for the chain conventions. */
  chains: Float32Array[];
}
