export type PhysicalMaterialTextureField =
  | 'map'
  | 'normalMap'
  | 'roughnessMap'
  | 'metalnessMap'
  | 'clearcoatNormalMap'
  | 'pomHeightMap';

/**
 * Capabilities + intents a host (Geotoy / level editor) supplies to the shared `MaterialForm`.
 * Flags gate genuinely host-specific affordances; the callbacks are routed to the host's own
 * shell (texture picker, shader-editor view, geoscript re-bake, etc.).
 */
export interface MaterialEditorHost {
  /** Geotoy references materials by name; the level editor keys by record id and hides this. */
  showName: boolean;
  /** Save/share to the geotoy material library (needs auth). */
  showSaveToLibrary: boolean;
  /** Geoscript UV-unwrap mapping mode + its viewer (needs a live geoscript mesh). */
  showUvUnwrap: boolean;
  /** Level-pipeline-only props (material class, fog, ambient, map-disable, opacity/transmission/ior). */
  showLevelProps: boolean;
  onpicktexture: (field: PhysicalMaterialTextureField) => void;
  onconverttype: (to: 'customShader' | 'customBasicShader') => void;
  oneditshaders: () => void;
  onviewuvmappings: () => void;
  onsavetolibrary: () => void;
  rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => void;
}
