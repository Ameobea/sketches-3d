import * as THREE from 'three';
import { mount, unmount } from 'svelte';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';

import type { Viz } from 'src/viz';
import type { BulletPhysics } from 'src/viz/collision';
import type { LevelDef, MaterialDef, ObjectDef } from './types';
import type { LevelObject } from './loadLevelDef';
import { buildMaterial } from './buildMaterial';
import LevelEditorPanel from './LevelEditorPanel.svelte';
import LevelMaterialEditor from './LevelMaterialEditor.svelte';

type TransformMode = 'translate' | 'rotate' | 'scale';

interface TransformSnapshot {
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

type UndoEntry =
  | { type: 'transform'; levelObj: LevelObject; before: TransformSnapshot; after: TransformSnapshot }
  | { type: 'add'; levelObj: LevelObject; snapshot: TransformSnapshot }
  | { type: 'delete'; levelObj: LevelObject; snapshot: TransformSnapshot };

/**
 * Captures the relative change of a transform operation so it can be replayed
 * on a different object via Shift+R (similar to Blender's "repeat last").
 */
interface ReplayableTransformDelta {
  positionDelta: [number, number, number];
  rotationDelta: [number, number, number];
  scaleFactor: [number, number, number];
}

interface ClipboardEntry {
  assetId: string;
  def: ObjectDef;
}

const MAX_UNDO = 50;

const PLACEHOLDER_MAT = new THREE.MeshStandardMaterial({ color: 0x888888 });

class LevelEditor {
  private viz: Viz;
  private levelName: string;
  private levelDef: LevelDef;
  private prototypes: Map<string, THREE.Object3D>;
  private builtMaterials: Map<string, THREE.Material>;
  private loadedTextures: Map<string, THREE.Texture>;

  private isEditMode = false;
  private orbitControls: OrbitControls | null = null;
  private transformControls: TransformControls | null = null;
  private selectedObject: LevelObject | null = null;
  private transformMode: TransformMode = 'translate';

  private raycaster = new THREE.Raycaster();
  private selectableMeshes: THREE.Mesh[] = [];
  private meshToLevelObject = new Map<THREE.Mesh, LevelObject>();
  private allLevelObjects: LevelObject[];

  // Distinguish clicks from drags — skip raycast if pointer moved significantly
  private pointerDownPos = new THREE.Vector2();
  private pointerMoved = false;

  // Undo / redo
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];
  private dragStartSnapshot: TransformSnapshot | null = null;

  // Copy / paste
  private clipboard: ClipboardEntry | null = null;

  // Repeat last action (Shift+R)
  private lastReplayableAction: ReplayableTransformDelta | null = null;

  // UI panel (reactive state for Svelte component)
  private panelState = $state({
    selectedObjectId: null as string | null,
    selectedMaterialId: null as string | null,
    materialEditorOpen: false,
  });
  private panelComponent: Record<string, any> | null = null;
  private panelTarget: HTMLDivElement | null = null;

  // Material editor
  private materialEditorComponent: Record<string, any> | null = null;
  private materialEditorTarget: HTMLDivElement | null = null;
  private materialSaveTimers = new Map<string, ReturnType<typeof setTimeout>>();

  constructor(
    viz: Viz,
    objects: LevelObject[],
    levelName: string,
    prototypes: Map<string, THREE.Object3D>,
    builtMaterials: Map<string, THREE.Material>,
    loadedTextures: Map<string, THREE.Texture>,
    levelDef: LevelDef
  ) {
    this.viz = viz;
    this.levelName = levelName;
    this.levelDef = levelDef;
    this.prototypes = prototypes;
    this.builtMaterials = builtMaterials;
    this.loadedTextures = loadedTextures;
    this.allLevelObjects = objects;

    for (const levelObj of objects) {
      this.registerMeshes(levelObj);
    }

    window.addEventListener('keydown', this.onKeyDown);
    viz.registerDestroyedCb(() => this.destroy());
  }

  private registerMeshes(levelObj: LevelObject) {
    levelObj.object.traverse(child => {
      if (child instanceof THREE.Mesh) {
        this.selectableMeshes.push(child);
        this.meshToLevelObject.set(child, levelObj);
      }
    });
  }

  private unregisterMeshes(levelObj: LevelObject) {
    levelObj.object.traverse(child => {
      if (child instanceof THREE.Mesh) {
        const idx = this.selectableMeshes.indexOf(child);
        if (idx !== -1) this.selectableMeshes.splice(idx, 1);
        this.meshToLevelObject.delete(child);
      }
    });
  }

  private snapshotTransform(obj: THREE.Object3D): TransformSnapshot {
    const r = obj.rotation;
    return {
      position: obj.position.toArray() as [number, number, number],
      rotation: [r.x, r.y, r.z],
      scale: obj.scale.toArray() as [number, number, number],
    };
  }

  private applySnapshot(obj: THREE.Object3D, snap: TransformSnapshot) {
    obj.position.fromArray(snap.position);
    obj.rotation.set(snap.rotation[0], snap.rotation[1], snap.rotation[2]);
    obj.scale.fromArray(snap.scale);
  }

  private onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      this.toggleEditMode();
      return;
    }

    if (!this.isEditMode) return;

    // Don't fire editor binds when the user is typing in an input or textarea.
    const activeTag = (document.activeElement as HTMLElement | null)?.tagName?.toLowerCase();
    const isTypingInput = activeTag === 'input' || activeTag === 'textarea';

    // Undo / Redo (allow even from inputs so browser-native text undo is not pre-empted —
    // we only handle it when focus is NOT in a text field).
    if (!isTypingInput) {
      if (e.key === 'z' && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
        e.preventDefault();
        this.undo();
        return;
      }
      if (
        ((e.key === 'z' || e.key === 'Z') && (e.ctrlKey || e.metaKey) && e.shiftKey) ||
        (e.key === 'y' && (e.ctrlKey || e.metaKey))
      ) {
        e.preventDefault();
        this.redo();
        return;
      }
    }

    // Copy / Paste
    if (e.key === 'c' && (e.ctrlKey || e.metaKey) && !isTypingInput) {
      if (this.selectedObject) {
        this.clipboard = { assetId: this.selectedObject.assetId, def: this.selectedObject.def };
      }
      return;
    }
    if (e.key === 'v' && (e.ctrlKey || e.metaKey) && !isTypingInput) {
      if (this.clipboard) {
        e.preventDefault();
        void this.pasteObject();
      }
      return;
    }

    if (isTypingInput) return;

    if (e.key === 'g' || e.key === 'G') {
      this.setTransformMode('translate');
    } else if (e.key === 'R' && e.shiftKey) {
      // Shift+R: repeat last transform action
      this.replayLastAction();
    } else if (e.key === 'r') {
      this.setTransformMode('rotate');
    } else if (e.key === 's' || e.key === 'S') {
      this.setTransformMode('scale');
    } else if (e.key === 'Escape') {
      this.deselect();
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      if (this.selectedObject) {
        e.preventDefault();
        this.deleteObject(this.selectedObject);
      }
    }
  };

  private setTransformMode(mode: TransformMode) {
    this.transformMode = mode;
    this.transformControls?.setMode(mode);
  }

  private toggleEditMode() {
    if (this.isEditMode) {
      this.exitEditMode();
    } else {
      this.enterEditMode();
    }
  }

  private enterEditMode() {
    this.isEditMode = true;

    this.viz.controlState.movementEnabled = false;
    this.viz.controlState.cameraControlEnabled = false;

    if (document.pointerLockElement) {
      document.exitPointerLock();
    }

    const playerPos = this.viz.fpCtx?.playerStateGetters.getPlayerPos();
    const target = playerPos
      ? new THREE.Vector3(playerPos[0], playerPos[1], playerPos[2])
      : new THREE.Vector3();

    const ORBIT_DISTANCE = 20;
    const lookDir = this.viz.camera.getWorldDirection(new THREE.Vector3());
    this.viz.camera.position.copy(target).addScaledVector(lookDir, -ORBIT_DISTANCE);

    this.orbitControls = new OrbitControls(this.viz.camera, this.viz.renderer.domElement);
    this.orbitControls.enableDamping = true;
    this.orbitControls.dampingFactor = 0.1;
    this.orbitControls.target.copy(target);
    this.orbitControls.update();

    this.transformControls = new TransformControls(this.viz.camera, this.viz.renderer.domElement);
    this.transformControls.setMode(this.transformMode);
    this.transformControls.addEventListener('dragging-changed', (e: any) => {
      if (this.orbitControls) {
        this.orbitControls.enabled = !e.value;
      }

      if (e.value) {
        if (this.selectedObject) {
          this.dragStartSnapshot = this.snapshotTransform(this.selectedObject.object);
        }
      } else {
        if (this.selectedObject && this.dragStartSnapshot) {
          const after = this.snapshotTransform(this.selectedObject.object);
          const before = this.dragStartSnapshot;
          this.pushUndo({
            type: 'transform',
            levelObj: this.selectedObject,
            before,
            after,
          });
          this.dragStartSnapshot = null;

          // Store delta for Shift+R replay
          this.lastReplayableAction = {
            positionDelta: [
              after.position[0] - before.position[0],
              after.position[1] - before.position[1],
              after.position[2] - before.position[2],
            ],
            rotationDelta: [
              after.rotation[0] - before.rotation[0],
              after.rotation[1] - before.rotation[1],
              after.rotation[2] - before.rotation[2],
            ],
            scaleFactor: [
              before.scale[0] !== 0 ? after.scale[0] / before.scale[0] : 1,
              before.scale[1] !== 0 ? after.scale[1] / before.scale[1] : 1,
              before.scale[2] !== 0 ? after.scale[2] / before.scale[2] : 1,
            ],
          };

          this.saveTransform(this.selectedObject);
          this.syncPhysics(this.selectedObject);
        }
      }
    });
    this.viz.scene.add(this.transformControls);

    this.viz.registerBeforeRenderCb(this.tickOrbitControls);

    const canvas = this.viz.renderer.domElement;
    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.onPointerUp);

    this.createPanel();
  }

  private exitEditMode() {
    this.isEditMode = false;

    this.deselect();
    this.closeMaterialEditor();

    this.viz.controlState.movementEnabled = true;
    this.viz.controlState.cameraControlEnabled = true;

    this.viz.unregisterBeforeRenderCb(this.tickOrbitControls);
    this.orbitControls?.dispose();
    this.orbitControls = null;

    if (this.transformControls) {
      this.viz.scene.remove(this.transformControls);
      this.transformControls.dispose();
      this.transformControls = null;
    }

    const canvas = this.viz.renderer.domElement;
    canvas.removeEventListener('pointerdown', this.onPointerDown);
    canvas.removeEventListener('pointermove', this.onPointerMove);
    canvas.removeEventListener('pointerup', this.onPointerUp);

    this.destroyPanel();
  }

  // ---------------------------------------------------------------------------
  // UI panel
  // ---------------------------------------------------------------------------

  private createPanel() {
    const target = document.createElement('div');
    document.body.appendChild(target);
    this.panelTarget = target;

    const state = this.panelState;

    this.panelComponent = mount(LevelEditorPanel, {
      target,
      props: {
        assetIds: Object.keys(this.levelDef.assets),
        materialIds: Object.keys(this.levelDef.materials ?? {}),
        get selectedObjectId(): string | null {
          return state.selectedObjectId;
        },
        get selectedMaterialId(): string | null {
          return state.selectedMaterialId;
        },
        get materialEditorOpen(): boolean {
          return state.materialEditorOpen;
        },
        onadd: (assetId: string, materialId: string | undefined) => this.onAddClick(assetId, materialId),
        onmaterialchange: (matId: string | null) => this.onObjectMaterialChange(matId),
        ontoggleMaterialEditor: () => {
          if (this.panelState.materialEditorOpen) {
            this.closeMaterialEditor();
          } else {
            this.openMaterialEditor(this.selectedObject?.def?.material ?? null);
          }
        },
      },
    });
  }

  private destroyPanel() {
    if (this.panelComponent) {
      unmount(this.panelComponent);
      this.panelComponent = null;
    }
    if (this.panelTarget) {
      this.panelTarget.remove();
      this.panelTarget = null;
    }
  }

  private updateSelectedLabel() {
    this.panelState.selectedObjectId = this.selectedObject?.id ?? null;
    this.panelState.selectedMaterialId = this.selectedObject?.def?.material ?? null;
  }

  // ---------------------------------------------------------------------------
  // Material editor
  // ---------------------------------------------------------------------------

  private openMaterialEditor(initialSelectedId?: string | null) {
    const target = document.createElement('div');
    document.body.appendChild(target);
    this.materialEditorTarget = target;

    this.materialEditorComponent = mount(LevelMaterialEditor, {
      target,
      props: {
        materials: this.levelDef.materials ?? {},
        textureKeys: Object.keys(this.levelDef.textures ?? {}),
        initialSelectedId: initialSelectedId ?? null,
        onchange: (id: string, def: MaterialDef) => this.onMaterialChange(id, def),
        onadd: (id: string, def: MaterialDef) => this.onMaterialAdd(id, def),
        ondelete: (id: string) => this.onMaterialDelete(id),
      },
    });
    this.panelState.materialEditorOpen = true;
  }

  private closeMaterialEditor() {
    if (this.materialEditorComponent) {
      unmount(this.materialEditorComponent);
      this.materialEditorComponent = null;
    }
    if (this.materialEditorTarget) {
      this.materialEditorTarget.remove();
      this.materialEditorTarget = null;
    }
    this.panelState.materialEditorOpen = false;
  }

  private remountMaterialEditor(newSelectedId?: string | null) {
    this.closeMaterialEditor();
    this.openMaterialEditor(newSelectedId);
  }

  private onMaterialChange(id: string, def: MaterialDef) {
    this.levelDef.materials![id] = def;

    const newMat = buildMaterial(def, this.loadedTextures);
    this.builtMaterials.get(id)?.dispose();
    this.builtMaterials.set(id, newMat);

    for (const levelObj of this.allLevelObjects) {
      if (levelObj.def.material === id) {
        levelObj.object.traverse(child => {
          if (child instanceof THREE.Mesh) {
            child.material = newMat;
          }
        });
      }
    }

    this.scheduleMaterialSave(id, def);
  }

  private scheduleMaterialSave(id: string, def: MaterialDef) {
    const existing = this.materialSaveTimers.get(id);
    if (existing !== undefined) clearTimeout(existing);
    const timer = setTimeout(() => {
      this.materialSaveTimers.delete(id);
      void this.saveMaterial(id, def);
    }, 500);
    this.materialSaveTimers.set(id, timer);
  }

  private saveMaterial = async (id: string, def: MaterialDef) => {
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

  private deleteMaterial = async (id: string) => {
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

  private onMaterialAdd(id: string, def: MaterialDef) {
    this.levelDef.materials ??= {};
    this.levelDef.materials[id] = def;

    const newMat = buildMaterial(def, this.loadedTextures);
    this.builtMaterials.set(id, newMat);

    void this.saveMaterial(id, def);
    this.remountMaterialEditor(id);
  }

  private onMaterialDelete(id: string) {
    delete this.levelDef.materials![id];

    this.builtMaterials.get(id)?.dispose();
    this.builtMaterials.delete(id);

    for (const levelObj of this.allLevelObjects) {
      if (levelObj.def.material === id) {
        levelObj.object.traverse(child => {
          if (child instanceof THREE.Mesh) {
            child.material = PLACEHOLDER_MAT;
          }
        });
      }
    }

    void this.deleteMaterial(id);
    this.remountMaterialEditor();
  }

  // ---------------------------------------------------------------------------
  // Orbit controls tick
  // ---------------------------------------------------------------------------

  private tickOrbitControls = () => {
    this.orbitControls?.update();
  };

  // ---------------------------------------------------------------------------
  // Pointer / raycast
  // ---------------------------------------------------------------------------

  private onPointerDown = (e: PointerEvent) => {
    this.pointerDownPos.set(e.clientX, e.clientY);
    this.pointerMoved = false;
  };

  private onPointerMove = (e: PointerEvent) => {
    const dx = e.clientX - this.pointerDownPos.x;
    const dy = e.clientY - this.pointerDownPos.y;
    if (dx * dx + dy * dy > 16) {
      this.pointerMoved = true;
    }
  };

  private onPointerUp = (e: PointerEvent) => {
    if (!this.pointerMoved && e.button === 0) {
      this.doRaycast(e);
    }
  };

  private doRaycast(e: PointerEvent) {
    const rect = this.viz.renderer.domElement.getBoundingClientRect();
    const mouse = new THREE.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );

    this.raycaster.setFromCamera(mouse, this.viz.camera);
    const hits = this.raycaster.intersectObjects(this.selectableMeshes, false);

    if (hits.length > 0) {
      const levelObj = this.meshToLevelObject.get(hits[0].object as THREE.Mesh);
      if (levelObj) {
        this.select(levelObj);
        return;
      }
    }

    this.deselect();
  }

  // ---------------------------------------------------------------------------
  // Select / deselect
  // ---------------------------------------------------------------------------

  private select(levelObj: LevelObject) {
    this.selectedObject = levelObj;
    this.transformControls?.attach(levelObj.object);
    this.updateSelectedLabel();
    if (this.panelState.materialEditorOpen && this.materialEditorComponent && levelObj.def.material) {
      (this.materialEditorComponent as any).setSelectedId(levelObj.def.material);
    }
  }

  private deselect() {
    this.selectedObject = null;
    this.transformControls?.detach();
    this.updateSelectedLabel();
  }

  // ---------------------------------------------------------------------------
  // Undo / redo helpers
  // ---------------------------------------------------------------------------

  private pushUndo(entry: UndoEntry) {
    this.undoStack.push(entry);
    if (this.undoStack.length > MAX_UNDO) this.undoStack.shift();
    this.redoStack.length = 0;
  }

  private undo() {
    const entry = this.undoStack.pop();
    if (!entry) return;
    this.redoStack.push(entry);
    this.applyUndoEntry(entry, 'undo');
  }

  private redo() {
    const entry = this.redoStack.pop();
    if (!entry) return;
    this.undoStack.push(entry);
    this.applyUndoEntry(entry, 'redo');
  }

  /**
   * Replay the last transform action on the currently selected object (Shift+R).
   * The stored delta is applied additively for position/rotation and
   * multiplicatively for scale, mirroring Blender's "repeat last" behaviour.
   */
  private replayLastAction() {
    if (!this.lastReplayableAction || !this.selectedObject) return;

    const delta = this.lastReplayableAction;
    const obj = this.selectedObject.object;
    const before = this.snapshotTransform(obj);

    const after: TransformSnapshot = {
      position: [
        before.position[0] + delta.positionDelta[0],
        before.position[1] + delta.positionDelta[1],
        before.position[2] + delta.positionDelta[2],
      ],
      rotation: [
        before.rotation[0] + delta.rotationDelta[0],
        before.rotation[1] + delta.rotationDelta[1],
        before.rotation[2] + delta.rotationDelta[2],
      ],
      scale: [
        before.scale[0] * delta.scaleFactor[0],
        before.scale[1] * delta.scaleFactor[1],
        before.scale[2] * delta.scaleFactor[2],
      ],
    };

    this.applySnapshot(obj, after);
    this.pushUndo({
      type: 'transform',
      levelObj: this.selectedObject,
      before,
      after,
    });
    this.saveTransform(this.selectedObject);
    this.syncPhysics(this.selectedObject);
  }

  private applyUndoEntry(entry: UndoEntry, direction: 'undo' | 'redo') {
    if (entry.type === 'transform') {
      const snap = direction === 'undo' ? entry.before : entry.after;
      this.applySnapshot(entry.levelObj.object, snap);
      this.select(entry.levelObj);
      this.saveTransform(entry.levelObj);
      this.syncPhysics(entry.levelObj);
    } else if (entry.type === 'add') {
      if (direction === 'undo') {
        // Un-add: remove from scene and server
        this.removeFromScene(entry.levelObj);
        this.sendDelete(entry.levelObj.id);
      } else {
        // Re-add: restore to scene and server
        this.addToScene(entry.levelObj, entry.snapshot);
        this.sendRestore(entry.levelObj, entry.snapshot);
      }
    } else {
      // type === 'delete'
      if (direction === 'undo') {
        // Un-delete: restore to scene and server
        this.addToScene(entry.levelObj, entry.snapshot);
        this.sendRestore(entry.levelObj, entry.snapshot);
      } else {
        // Re-delete: remove from scene and server
        this.removeFromScene(entry.levelObj);
        this.sendDelete(entry.levelObj.id);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Scene add / remove (without server calls)
  // ---------------------------------------------------------------------------

  private removeFromScene(levelObj: LevelObject) {
    if (this.selectedObject === levelObj) this.deselect();
    this.unregisterMeshes(levelObj);
    this.removePhysics(levelObj);
    this.viz.scene.remove(levelObj.object);
    const idx = this.allLevelObjects.indexOf(levelObj);
    if (idx !== -1) this.allLevelObjects.splice(idx, 1);
  }

  private addToScene(levelObj: LevelObject, snapshot: TransformSnapshot) {
    this.applySnapshot(levelObj.object, snapshot);
    this.viz.scene.add(levelObj.object);
    this.allLevelObjects.push(levelObj);
    this.registerMeshes(levelObj);
    if (this.viz.fpCtx) this.syncPhysics(levelObj);
    this.select(levelObj);
  }

  // ---------------------------------------------------------------------------
  // Delete object (user-initiated)
  // ---------------------------------------------------------------------------

  private deleteObject(levelObj: LevelObject) {
    const snapshot = this.snapshotTransform(levelObj.object);
    this.removeFromScene(levelObj);
    // Only purge stale transform entries; the delete entry itself is the new action
    this.undoStack = this.undoStack.filter(e => !(e.type === 'transform' && e.levelObj === levelObj));
    this.redoStack = this.redoStack.filter(e => !(e.type === 'transform' && e.levelObj === levelObj));
    this.pushUndo({ type: 'delete', levelObj, snapshot });
    this.sendDelete(levelObj.id);
  }

  // ---------------------------------------------------------------------------
  // Add / paste object (user-initiated)
  // ---------------------------------------------------------------------------

  private async onAddClick(assetId: string, materialId: string | undefined) {
    if (!this.prototypes.has(assetId)) {
      console.warn(`[LevelEditor] No prototype for asset "${assetId}" — asset may not be loaded yet`);
      return;
    }

    const round = (n: number) => Math.round(n * 10000) / 10000;
    const orbitTarget = this.orbitControls?.target ?? new THREE.Vector3();
    const position: [number, number, number] = [
      round(orbitTarget.x),
      round(orbitTarget.y),
      round(orbitTarget.z),
    ];

    const newDef = await this.sendAdd({ asset: assetId, material: materialId, position });
    if (newDef) this.finalizeSpawn(assetId, newDef);
  }

  private async pasteObject() {
    if (!this.clipboard) return;
    const { assetId, def } = this.clipboard;

    if (!this.prototypes.has(assetId)) {
      console.warn(`[LevelEditor] No prototype for asset "${assetId}" — asset may not be loaded yet`);
      return;
    }

    // Offset slightly so the copy doesn't land exactly on the original.
    const srcPos = def.position ?? [0, 0, 0];
    const position: [number, number, number] = [srcPos[0], srcPos[1] + 0.5, srcPos[2]];

    const newDef = await this.sendAdd({
      asset: assetId,
      material: def.material,
      position,
      rotation: def.rotation,
      scale: def.scale,
    });
    if (newDef) this.finalizeSpawn(assetId, newDef);
  }

  /** Shared final step for add and paste: clone prototype, place in scene, push undo, select. */
  private finalizeSpawn(assetId: string, newDef: ObjectDef) {
    const prototype = this.prototypes.get(assetId)!;
    const clone = prototype.clone();
    const [px = 0, py = 0, pz = 0] = newDef.position ?? [];
    const [rx = 0, ry = 0, rz = 0] = newDef.rotation ?? [];
    const [sx = 1, sy = 1, sz = 1] = newDef.scale ?? [];
    clone.position.set(px, py, pz);
    clone.rotation.set(rx, ry, rz, 'YXZ');
    clone.scale.set(sx, sy, sz);
    clone.userData = { levelDefId: newDef.id };

    if (newDef.material) {
      const mat = this.builtMaterials.get(newDef.material) ?? PLACEHOLDER_MAT;
      clone.traverse(child => {
        if (child instanceof THREE.Mesh) child.material = mat;
      });
    }

    clone.traverse(child => {
      if (child instanceof THREE.Mesh) {
        child.castShadow = true;
        child.receiveShadow = true;
      }
    });

    const levelObj: LevelObject = { id: newDef.id, assetId, object: clone, def: newDef };
    const snapshot = this.snapshotTransform(clone);

    this.viz.scene.add(clone);
    this.allLevelObjects.push(levelObj);
    this.registerMeshes(levelObj);
    if (this.viz.fpCtx) this.syncPhysics(levelObj);

    this.pushUndo({ type: 'add', levelObj, snapshot });
    this.select(levelObj);
  }

  // ---------------------------------------------------------------------------
  // Object material change (user-initiated)
  // ---------------------------------------------------------------------------

  private onObjectMaterialChange(matId: string | null) {
    const levelObj = this.selectedObject;
    if (!levelObj) return;

    if (matId) {
      levelObj.def.material = matId;
      const mat = this.builtMaterials.get(matId) ?? PLACEHOLDER_MAT;
      levelObj.object.traverse(child => {
        if (child instanceof THREE.Mesh) child.material = mat;
      });
    } else {
      delete levelObj.def.material;
      levelObj.object.traverse(child => {
        if (child instanceof THREE.Mesh) child.material = PLACEHOLDER_MAT;
      });
    }

    this.panelState.selectedMaterialId = matId;
    void this.saveMaterialAssignment(levelObj.id, matId);
  }

  // ---------------------------------------------------------------------------
  // Server calls
  // ---------------------------------------------------------------------------

  private sendAdd = async (body: {
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

  private saveMaterialAssignment = async (id: string, material: string | null) => {
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

  private saveTransform = async (levelObj: LevelObject) => {
    const { object } = levelObj;
    const round = (n: number) => Math.round(n * 10000) / 10000;

    const body = {
      id: levelObj.id,
      position: object.position.toArray().map(round) as [number, number, number],
      rotation: [object.rotation.x, object.rotation.y, object.rotation.z].map(round) as [
        number,
        number,
        number,
      ],
      scale: object.scale.toArray().map(round) as [number, number, number],
    };

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

  private sendDelete = async (id: string) => {
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

  private sendRestore = (levelObj: LevelObject, snapshot: TransformSnapshot) => {
    const round = (n: number) => Math.round(n * 10000) / 10000;
    void this.sendAdd({
      id: levelObj.id,
      asset: levelObj.assetId,
      material: levelObj.def.material,
      position: snapshot.position.map(round) as [number, number, number],
      rotation: snapshot.rotation.map(round) as [number, number, number],
      scale: snapshot.scale.map(round) as [number, number, number],
    });
  };

  // ---------------------------------------------------------------------------
  // Physics
  // ---------------------------------------------------------------------------

  private syncPhysics(levelObj: LevelObject) {
    const fpCtx: BulletPhysics | undefined = this.viz.fpCtx;
    if (!fpCtx) return;

    levelObj.object.traverse(child => {
      if (!(child instanceof THREE.Mesh)) return;

      if (child.userData.rigidBody) {
        fpCtx.removeCollisionObject(child.userData.rigidBody, child.name);
        child.userData.rigidBody = undefined;
      } else if (child.userData.collisionObj) {
        fpCtx.removeCollisionObject(child.userData.collisionObj, child.name);
        child.userData.collisionObj = undefined;
      }

      child.updateWorldMatrix(true, false);

      const origPos = child.position.clone();
      const origQuat = child.quaternion.clone();
      const origScale = child.scale.clone();

      child.matrixWorld.decompose(child.position, child.quaternion, child.scale);
      fpCtx.addTriMesh(child);

      child.position.copy(origPos);
      child.quaternion.copy(origQuat);
      child.scale.copy(origScale);
    });
  }

  private removePhysics(levelObj: LevelObject) {
    const fpCtx: BulletPhysics | undefined = this.viz.fpCtx;
    if (!fpCtx) return;

    levelObj.object.traverse(child => {
      if (!(child instanceof THREE.Mesh)) return;

      if (child.userData.rigidBody) {
        fpCtx.removeCollisionObject(child.userData.rigidBody, child.name);
        child.userData.rigidBody = undefined;
      } else if (child.userData.collisionObj) {
        fpCtx.removeCollisionObject(child.userData.collisionObj, child.name);
        child.userData.collisionObj = undefined;
      }
    });
  }

  // ---------------------------------------------------------------------------
  // Cleanup
  // ---------------------------------------------------------------------------

  private destroy() {
    window.removeEventListener('keydown', this.onKeyDown);
    if (this.isEditMode) {
      this.exitEditMode();
    }
  }
}

export const initLevelEditor = (
  viz: Viz,
  objects: LevelObject[],
  levelName: string,
  prototypes: Map<string, THREE.Object3D>,
  builtMaterials: Map<string, THREE.Material>,
  loadedTextures: Map<string, THREE.Texture>,
  levelDef: LevelDef
): LevelEditor =>
  new LevelEditor(viz, objects, levelName, prototypes, builtMaterials, loadedTextures, levelDef);
