import type { RenderedControl } from 'src/geoscript/runner/types';
import type { SplinePanelCtx } from 'src/geoscript/controlsUi';
import type { AssetLibFolder } from './assetLibTypes';
import type { LevelLight, LevelSceneNode } from './loadLevelDef';
import type { TransformSnapshot } from './TransformHandler';
import type { InputValueJson, LightDef } from './types';

/** Material-mapping surface for a selected geotoy composition node. */
export interface CompositionMaterialInfo {
  /** How many placements share this asset's mapping (an edit affects them all). */
  placementCount: number;
  /** One row per geotoy material name; `mappedTo` is the explicit override, or null (composition default). */
  rows: { geotoyName: string; mappedTo: string | null }[];
}

/** Parametric-inputs surface for a selected placement of a parametric asset. */
export interface ObjectInputsInfo {
  /** `input_*` sites from the placement's variant run (or the base asset's as fallback). */
  controls: RenderedControl[];
  /** Effective values by bare input name: asset-level `inputs` overlaid with the object's. */
  overrides: Record<string, InputValueJson>;
}

/** One `gizmo(...)` handle of the selected placement, for the handles panel. */
export interface GizmoHandleRowInfo {
  /** `def.inputs` override key (bare handle name or `module/handle`). */
  key: string;
  /** Declaring composition node name; null when unambiguous (single module). */
  module: string | null;
  handleId: string;
  kind: 'vec3' | 'transform';
  color: string;
  readout: string;
  overridden: boolean;
  armed: boolean;
}

export interface LevelEditorPanelActions {
  selectNode(node: LevelSceneNode, ctrlKey: boolean): void;
  selectLight(light: LevelLight): void;
  addObject(assetId: string, materialId: string | undefined): void;
  addLibraryObject(libPath: string, materialId: string | undefined): void;
  addGroup(): void;
  addLight(lightType: LightDef['type']): void;
  rename(newId: string): void;
  changeMaterial(matId: string | null): void;
  /** Set (matId) or clear (null → composition default) a composition material-map override. */
  mapCompositionMaterial(geotoyName: string, matId: string | null): void;
  /** Set (or clear, with undefined) a per-object override on the selected placement (live-rebuilds it). */
  setObjectInput(handleId: string, value: InputValueJson | undefined): void;
  /** Arm (or disarm, with null) a gizmo handle on the selected placement. */
  armGizmoHandle(key: string | null): void;
  /** Clear a gizmo handle's per-object override. */
  resetGizmoHandle(key: string): void;
  applyTransform(snap: Partial<TransformSnapshot>): void;
  applyLightPosition(pos: [number, number, number]): void;
  applyLightProperty(update: Partial<LightDef>): void;
  deleteSelection(): void;
  deleteLight(): void;
  toggleMaterialEditor(): void;
  convertToCsg(): void;
  groupSelected?(): void;
  reparent?(parentId: string | null): void;
  recenterGroupOrigin(): void;
}

export interface LevelEditorPanelViewState {
  assetIds: string[];
  materialIds: string[];
  libFolders: AssetLibFolder[];
  materialLibFolders: AssetLibFolder[];
  rootNodes: LevelSceneNode[];
  lights: LevelLight[];
  selectedNodeIds: string[];
  selectedNodeId: string | null;
  treeVersion: number;
  selectedMaterialId: string | null;
  selectedLightId: string | null;
  selectedLightDef: LightDef | null;
  lightPosition: [number, number, number];
  isGroupSelected: boolean;
  isGeneratedSelected: boolean;
  /** Non-null when a single geotoy composition node is selected; drives the material-map panel. */
  compositionMaterials: CompositionMaterialInfo | null;
  /** Non-null when the selected placement's asset declares `input_*` controls. */
  objectInputs: ObjectInputsInfo | null;
  /** Non-null when the selected placement's asset reported `gizmo(...)` handles. */
  gizmoHandles: GizmoHandleRowInfo[] | null;
  /** Spline-editing bridge for the selected placement's `input_spline` controls. */
  splineCtx: SplinePanelCtx;
  materialEditorOpen: boolean;
  isCsgAsset: boolean;
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
  canGroupSelected?: boolean;
  canConvertSelectedToCsg?: boolean;
  canRecenterGroupOrigin: boolean;
}
