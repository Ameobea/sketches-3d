import type * as THREE from 'three';
import { mount, unmount } from 'svelte';

import type { LevelDef, MaterialDef } from './types';
import type { LevelObject } from './loadLevelDef';
import type { LevelEditorApi } from './levelEditorApi';
import { buildMaterial } from './buildMaterial';
import { LEVEL_PLACEHOLDER_MAT, assignMaterial } from './levelObjectUtils';
import LevelMaterialEditor from './LevelMaterialEditor.svelte';

export class MaterialEditorController {
  private component: Record<string, any> | null = null;
  private target: HTMLDivElement | null = null;
  private saveTimers = new Map<string, ReturnType<typeof setTimeout>>();

  isOpen = false;

  constructor(
    private levelDef: LevelDef,
    private builtMaterials: Map<string, THREE.Material>,
    private loadedTextures: Map<string, THREE.Texture>,
    private allLevelObjects: LevelObject[],
    private api: LevelEditorApi
  ) {}

  open(initialSelectedId?: string | null) {
    this.close();

    const target = document.createElement('div');
    document.body.appendChild(target);
    this.target = target;

    this.component = mount(LevelMaterialEditor, {
      target,
      props: {
        materials: this.levelDef.materials ?? {},
        textureKeys: Object.keys(this.levelDef.textures ?? {}),
        initialSelectedId: initialSelectedId ?? null,
        onchange: (id: string, def: MaterialDef) => this.onChange(id, def),
        onadd: (id: string, def: MaterialDef) => this.onAdd(id, def),
        ondelete: (id: string) => this.onDelete(id),
      },
    });
    this.isOpen = true;
  }

  close() {
    if (this.component) {
      unmount(this.component);
      this.component = null;
    }
    if (this.target) {
      this.target.remove();
      this.target = null;
    }
    this.isOpen = false;
  }

  /** Forward selection changes from the main editor. */
  setSelectedId(id: string) {
    if (this.component) {
      (this.component as any).setSelectedId(id);
    }
  }

  private onChange(id: string, def: MaterialDef) {
    this.levelDef.materials![id] = def;

    const newMat = buildMaterial(def, this.loadedTextures);
    this.builtMaterials.get(id)?.dispose();
    this.builtMaterials.set(id, newMat);

    for (const levelObj of this.allLevelObjects) {
      if (levelObj.def.material === id) {
        assignMaterial(levelObj.object, newMat);
      }
    }

    this.scheduleSave(id, def);
  }

  private scheduleSave(id: string, def: MaterialDef) {
    const existing = this.saveTimers.get(id);
    if (existing !== undefined) clearTimeout(existing);
    const timer = setTimeout(() => {
      this.saveTimers.delete(id);
      void this.api.saveMaterial(id, def);
    }, 500);
    this.saveTimers.set(id, timer);
  }

  private onAdd(id: string, def: MaterialDef) {
    this.levelDef.materials ??= {};
    this.levelDef.materials[id] = def;

    const newMat = buildMaterial(def, this.loadedTextures);
    this.builtMaterials.set(id, newMat);

    void this.api.saveMaterial(id, def);
    // Remount to refresh the material list
    this.close();
    this.open(id);
  }

  private onDelete(id: string) {
    delete this.levelDef.materials![id];

    this.builtMaterials.get(id)?.dispose();
    this.builtMaterials.delete(id);

    for (const levelObj of this.allLevelObjects) {
      if (levelObj.def.material === id) {
        assignMaterial(levelObj.object, LEVEL_PLACEHOLDER_MAT);
      }
    }

    void this.api.deleteMaterial(id);
    this.close();
    this.open();
  }
}
