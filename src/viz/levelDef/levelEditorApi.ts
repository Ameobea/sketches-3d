import type { CsgTreeNode, ObjectDef } from './types';
import type { LevelObject, LevelSceneNode } from './loadLevelDef';

const round = (n: number) => Math.round(n * 10000) / 10000;

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

  sendRestore = (
    levelObj: LevelObject,
    snapshot: {
      position: [number, number, number];
      rotation: [number, number, number];
      scale: [number, number, number];
    }
  ) => {
    void this.sendAdd({
      id: levelObj.id,
      asset: levelObj.assetId,
      material: levelObj.def.material,
      position: snapshot.position.map(round) as [number, number, number],
      rotation: snapshot.rotation.map(round) as [number, number, number],
      scale: snapshot.scale.map(round) as [number, number, number],
    });
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
}
