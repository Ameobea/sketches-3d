import type { AssetLibFolder } from './assetLibTypes';
import type {
  CsgTreeNode,
  EditorBookmark,
  LightDef,
  LocationsFile,
  MaterialDef,
  ObjectDef,
  ObjectGroupDef,
  TextureDef,
} from './types';
import type { LevelSceneNode } from './loadLevelDef';
import type { RuntimeSubtree } from './editorStructuralTypes';
import type { TransformSnapshot } from './TransformHandler';
import { round } from './mathUtils';
import { nodeToDef } from './editorNodeFactory';

const JSON_HEADERS = { 'Content-Type': 'application/json' };

const jsonInit = (method: string, body: unknown): RequestInit => ({
  method,
  headers: JSON_HEADERS,
  body: JSON.stringify(body),
});

export class LevelEditorApi {
  constructor(private levelName: string) {}

  /** Fetch + ok-check, logging failures under `label`. Returns the Response, or null on failure. */
  private async fetchOk(label: string, path: string, init?: RequestInit): Promise<Response | null> {
    try {
      const res = await fetch(`/level_editor/${this.levelName}${path}`, init);
      if (res.ok) return res;
      console.error(`[LevelEditor] ${label} failed:`, res.status, await res.text());
    } catch (err) {
      console.error(`[LevelEditor] ${label} error:`, err);
    }
    return null;
  }

  /** As `fetchOk`, but parses and returns the JSON body (null on any failure). */
  private async reqJson<T>(label: string, path: string, init?: RequestInit): Promise<T | null> {
    const res = await this.fetchOk(label, path, init);
    if (!res) return null;
    try {
      return (await res.json()) as T;
    } catch (err) {
      console.error(`[LevelEditor] ${label} parse error:`, err);
      return null;
    }
  }

  saveTransform = async (node: LevelSceneNode) => {
    const { object, id, def } = node;

    const body = {
      id,
      position: object.position.toArray().map(round) as [number, number, number],
      rotation: [object.rotation.x, object.rotation.y, object.rotation.z].map(round) as [
        number,
        number,
        number,
      ],
      scale: object.scale.toArray().map(round) as [number, number, number],
    };

    // Write back to def so clipboard and other consumers see current values
    def.position = body.position;
    def.rotation = body.rotation;
    def.scale = body.scale;

    await this.fetchOk('save', '', jsonInit('PATCH', body));
  };

  sendAdd = (body: {
    asset: string;
    material?: string;
    position: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    id?: string;
    parentId?: string;
    index?: number;
  }): Promise<ObjectDef | null> => this.reqJson('add', '', jsonInit('POST', body));

  /** Paste a full node def (leaf or group) — the server assigns fresh ids and preserves every
   *  other field, so behaviors/parkour/userData/flags survive (unlike the minimal `sendAdd`). */
  sendPaste = (
    def: ObjectDef | ObjectGroupDef,
    parentId?: string,
    index?: number
  ): Promise<ObjectDef | ObjectGroupDef | null> =>
    this.reqJson('paste', '', jsonInit('POST', { type: 'group_paste', def, parentId, index }));

  sendAddGroup = (body: {
    position: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    id?: string;
  }): Promise<ObjectGroupDef | null> =>
    this.reqJson('add group', '', jsonInit('POST', { type: 'group', ...body }));

  sendDelete = async (id: string) => {
    await this.fetchOk('delete', '', jsonInit('DELETE', { id }));
  };

  restoreSubtree = async (subtree: RuntimeSubtree): Promise<void> => {
    const def = JSON.parse(JSON.stringify(nodeToDef(subtree.root))) as ObjectDef | ObjectGroupDef;
    def.position = subtree.transform.position.map(round) as [number, number, number];
    def.rotation = subtree.transform.rotation.map(round) as [number, number, number];
    def.scale = subtree.transform.scale.map(round) as [number, number, number];

    await this.fetchOk(
      'restore subtree',
      '',
      jsonInit('POST', {
        type: 'restore_subtree',
        def,
        parentId: subtree.placement.parent.type === 'group' ? subtree.placement.parent.groupId : undefined,
        index: subtree.placement.index,
      })
    );
  };

  renameNode = (id: string, newId: string): Promise<{ resolvedId: string } | null> =>
    this.reqJson('rename', '', jsonInit('PATCH', { id, newId }));

  saveMaterialAssignment = async (id: string, material: string | null) => {
    await this.fetchOk('material assignment save', '', jsonInit('PATCH', { id, material }));
  };

  /** Persist a placement's per-object `inputs` (undefined/empty clears the field). */
  saveInputs = async (id: string, inputs: Record<string, import('./types').InputValueJson> | undefined) => {
    await this.fetchOk('inputs save', '', jsonInit('PATCH', { id, inputs: inputs ?? null }));
  };

  saveCompositionMaterialMap = async (assetId: string, materialMap: Record<string, string>) => {
    await this.fetchOk(
      'composition material-map save',
      '/composition-material-map',
      jsonInit('PATCH', { assetId, materialMap })
    );
  };

  saveMaterial = async (id: string, def: import('./types').MaterialDef) => {
    await this.fetchOk('material save', '/materials', jsonInit('PUT', { name: id, def }));
  };

  deleteMaterial = async (id: string) => {
    await this.fetchOk('material delete', '/materials', jsonInit('DELETE', { name: id }));
  };

  fetchAssetLibrary = async (): Promise<AssetLibFolder[]> => {
    const data = await this.reqJson<{ folders: AssetLibFolder[] }>('asset library fetch', '/asset-library');
    return data?.folders ?? [];
  };

  registerLibraryAsset = (file: string): Promise<{ id: string; code: string } | null> =>
    this.reqJson('register library asset', '/assets', jsonInit('POST', { file }));

  fetchMaterialLibrary = async (): Promise<AssetLibFolder[]> => {
    const data = await this.reqJson<{ folders: AssetLibFolder[] }>(
      'material library fetch',
      '/material-library'
    );
    return data?.folders ?? [];
  };

  resolveLibraryMaterial = (
    libRef: string
  ): Promise<{ material: MaterialDef; textures: Record<string, TextureDef> } | null> =>
    this.reqJson('resolve library material', '/materials', jsonInit('POST', { libRef }));

  saveCsgTree = (assetName: string, tree: CsgTreeNode) => {
    void this.fetchOk('CSG tree save', '/csg', jsonInit('PATCH', { assetName, tree }));
  };

  addLight = (def: Omit<LightDef, 'id'> & { id?: string }): Promise<LightDef | null> =>
    this.reqJson('add light', '/lights', jsonInit('POST', def));

  saveLight = async (def: LightDef): Promise<void> => {
    await this.fetchOk('save light', '/lights', jsonInit('PATCH', def));
  };

  deleteLight = async (id: string): Promise<void> => {
    await this.fetchOk('delete light', '/lights', jsonInit('DELETE', { id }));
  };

  convertToCsg = (
    objectIds: string[]
  ): Promise<{ csgAssetName: string; tree: CsgTreeNode; primaryId: string; deletedIds: string[] } | null> =>
    this.reqJson('convert to CSG', '/csg', jsonInit('POST', { objectIds }));

  groupNodes = (nodeIds: string[], position: [number, number, number]): Promise<ObjectGroupDef | null> =>
    this.reqJson('group nodes', '', jsonInit('PUT', { nodeIds, position }));

  fetchLocations = (): Promise<LocationsFile | null> => this.reqJson('locations fetch', '/locations');

  saveBookmark = async (bookmark: EditorBookmark): Promise<void> => {
    await this.fetchOk('bookmark save', '/locations', jsonInit('PUT', bookmark));
  };

  reparentNodes = async (
    nodes: Array<{ id: string; transform: TransformSnapshot }>,
    parentId?: string,
    index?: number
  ): Promise<boolean> => {
    const res = await this.fetchOk(
      'reparent',
      '',
      jsonInit('POST', {
        type: 'reparent',
        nodes: nodes.map(node => ({
          id: node.id,
          position: node.transform.position.map(round) as [number, number, number],
          rotation: node.transform.rotation.map(round) as [number, number, number],
          scale: node.transform.scale.map(round) as [number, number, number],
        })),
        parentId,
        index,
      })
    );
    return res !== null;
  };
}
