import type { AssetLibFolder } from './assetLibTypes';
import type { CsgTreeNode, LightDef, ObjectDef, ObjectGroupDef } from './types';
import type { LevelSceneNode } from './loadLevelDef';
import type { RuntimeSubtree } from './editorStructuralTypes';
import type { TransformSnapshot } from './TransformHandler';
import { round } from './mathUtils';

export class LevelEditorApi {
  constructor(private levelName: string) {}

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

    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        console.error('[LevelEditor] save failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] save error:', err);
    }
  };

  sendAdd = async (body: {
    asset: string;
    material?: string;
    position: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    id?: string;
    parentId?: string;
    index?: number;
  }): Promise<ObjectDef | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!res.ok) {
        console.error('[LevelEditor] add failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] add error:', err);
      return null;
    }
  };

  sendPasteGroup = async (
    def: ObjectGroupDef,
    parentId?: string,
    index?: number
  ): Promise<ObjectGroupDef | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ type: 'group_paste', def, parentId, index }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] paste group failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] paste group error:', err);
      return null;
    }
  };

  sendAddGroup = async (body: {
    position: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    id?: string;
  }): Promise<ObjectGroupDef | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ type: 'group', ...body }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] add group failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] add group error:', err);
      return null;
    }
  };

  sendDelete = async (id: string) => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] delete failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] delete error:', err);
    }
  };

  restoreSubtree = async (subtree: RuntimeSubtree): Promise<void> => {
    const def = JSON.parse(JSON.stringify(subtree.root.def)) as ObjectDef | ObjectGroupDef;
    def.position = subtree.transform.position.map(round) as [number, number, number];
    def.rotation = subtree.transform.rotation.map(round) as [number, number, number];
    def.scale = subtree.transform.scale.map(round) as [number, number, number];

    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          type: 'restore_subtree',
          def,
          parentId: subtree.placement.parent.type === 'group' ? subtree.placement.parent.groupId : undefined,
          index: subtree.placement.index,
        }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] restore subtree failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] restore subtree error:', err);
    }
  };

  renameNode = async (id: string, newId: string): Promise<{ resolvedId: string } | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id, newId }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] rename failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] rename error:', err);
      return null;
    }
  };

  saveMaterialAssignment = async (id: string, material: string | null) => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id, material }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] material assignment save failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] material assignment save error:', err);
    }
  };

  saveMaterial = async (id: string, def: import('./types').MaterialDef) => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/materials`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: id, def }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] material save failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] material save error:', err);
    }
  };

  deleteMaterial = async (id: string) => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/materials`, {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: id }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] material delete failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] material delete error:', err);
    }
  };

  fetchAssetLibrary = async (): Promise<AssetLibFolder[]> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/asset-library`);
      if (!res.ok) {
        console.error('[LevelEditor] asset library fetch failed:', res.status, await res.text());
        return [];
      }
      const data = (await res.json()) as { folders: AssetLibFolder[] };
      return data.folders;
    } catch (err) {
      console.error('[LevelEditor] asset library fetch error:', err);
      return [];
    }
  };

  registerLibraryAsset = async (file: string): Promise<{ id: string; code: string } | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/assets`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ file }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] register library asset failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] register library asset error:', err);
      return null;
    }
  };

  saveCsgTree = (assetName: string, tree: CsgTreeNode) => {
    fetch(`/level_editor/${this.levelName}/csg`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ assetName, tree }),
    })
      .then(res => {
        if (!res.ok)
          res.text().then(t => console.error('[LevelEditor] CSG tree save failed:', res.status, t));
      })
      .catch(err => console.error('[LevelEditor] CSG tree save error:', err));
  };

  addLight = async (def: Omit<LightDef, 'id'> & { id?: string }): Promise<LightDef | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/lights`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(def),
      });
      if (!res.ok) {
        console.error('[LevelEditor] add light failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] add light error:', err);
      return null;
    }
  };

  saveLight = async (def: LightDef): Promise<void> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/lights`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(def),
      });
      if (!res.ok) {
        console.error('[LevelEditor] save light failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] save light error:', err);
    }
  };

  deleteLight = async (id: string): Promise<void> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/lights`, {
        method: 'DELETE',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] delete light failed:', res.status, await res.text());
      }
    } catch (err) {
      console.error('[LevelEditor] delete light error:', err);
    }
  };

  convertToCsg = async (objectId: string): Promise<{ csgAssetName: string; tree: CsgTreeNode } | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}/csg`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ objectId }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] convert to CSG failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] convert to CSG error:', err);
      return null;
    }
  };

  groupNodes = async (
    nodeIds: string[],
    position: [number, number, number]
  ): Promise<ObjectGroupDef | null> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ nodeIds, position }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] group nodes failed:', res.status, await res.text());
        return null;
      }
      return await res.json();
    } catch (err) {
      console.error('[LevelEditor] group nodes error:', err);
      return null;
    }
  };

  reparentNodes = async (
    nodes: Array<{ id: string; transform: TransformSnapshot }>,
    parentId?: string,
    index?: number
  ): Promise<boolean> => {
    try {
      const res = await fetch(`/level_editor/${this.levelName}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          type: 'reparent',
          nodes: nodes.map(node => ({
            id: node.id,
            position: node.transform.position.map(round) as [number, number, number],
            rotation: node.transform.rotation.map(round) as [number, number, number],
            scale: node.transform.scale.map(round) as [number, number, number],
          })),
          parentId,
          index,
        }),
      });
      if (!res.ok) {
        console.error('[LevelEditor] reparent failed:', res.status, await res.text());
        return false;
      }
      return true;
    } catch (err) {
      console.error('[LevelEditor] reparent error:', err);
      return false;
    }
  };
}
