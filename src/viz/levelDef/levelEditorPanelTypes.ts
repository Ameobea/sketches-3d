import type { AssetLibFolder } from './assetLibTypes';
import type { LevelLight, LevelSceneNode } from './loadLevelDef';
import type { TransformSnapshot } from './TransformHandler';
import type { LightDef } from './types';

/** Material-mapping surface for a selected geotoy composition node. */
export interface CompositionMaterialInfo {
  /** How many placements share this asset's mapping (an edit affects them all). */
  placementCount: number;
  /** One row per geotoy material name; `mappedTo` is the explicit override, or null (composition default). */
  rows: { geotoyName: string; mappedTo: string | null }[];
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
  materialEditorOpen: boolean;
  isCsgAsset: boolean;
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
  canGroupSelected?: boolean;
  canConvertSelectedToCsg?: boolean;
  canRecenterGroupOrigin: boolean;
}
