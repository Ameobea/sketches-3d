import * as THREE from 'three';

import type { GizmoValue, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import type { RenderedGizmo } from 'src/geoscript/runner/types';
import { decomposeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
import { GizmoGhosts, type GhostSpec } from 'src/viz/gizmos/gizmoGhosts';
import { gizmoColorForIndex } from 'src/viz/gizmos/gizmoPalette';
import type { HandleContext } from 'src/viz/gizmos/gizmoTypes';
import { HandleTarget } from 'src/viz/gizmos/targets';
import { composeInstance0World } from 'src/viz/scenes/geoscriptPlayground/treeOps';

import type { LevelEditor } from './LevelEditor.svelte';
import type { GizmoHandleRowInfo } from './levelEditorPanelTypes';
import type { LevelSceneNode } from './levelSceneTypes';
import { round } from './mathUtils';
import type { InputValueJson, ObjectDef } from './types';

interface ActiveTarget {
  node: LevelSceneNode;
  def: ObjectDef;
  gizmos: RenderedGizmo[];
  tree: TreeDef | null;
  moduleToNodeId: Record<string, string>;
}

interface HandleRow {
  /** `def.inputs` key: bare handle name, or `module/handle` when the bare name is ambiguous. */
  key: string;
  module: string;
  handleId: string;
  kind: 'vec3' | 'transform';
  color: string;
  gz: RenderedGizmo;
}

const _mat = new THREE.Matrix4();
const _mat2 = new THREE.Matrix4();
const _pos = new THREE.Vector3();

const roundVec3 = (v: readonly number[]): [number, number, number] => [round(v[0]), round(v[1]), round(v[2])];

const fmt = (n: number) => (Math.abs(n) < 1e-6 ? '0' : n.toFixed(2).replace(/\.?0+$/, ''));

/**
 * Gizmo-handle editing for the selected placement: renders ghost markers for every
 * `gizmo(...)` site the asset's last run reported, arms a handle on ghost/panel click
 * (shared transform gizmo via `HandleTarget`), and commits drags as per-object `inputs`
 * overrides through the normal param-variant rebuild path. Not an `EditorMode` — arming
 * is transient state within normal object editing, mirroring Geotoy's UX.
 */
export class GizmoHandlesController {
  private ghosts: GizmoGhosts | null = null;
  private current: ActiveTarget | null = null;
  private rows: HandleRow[] = [];
  private armedKey: string | null = null;
  /** Snapshot for the armed handle so a rebuild landing mid-drag can't move the anchor. */
  private armedContext: HandleContext | null = null;
  /** Values committed/previewed but not yet reflected in retained run data; keyed `nodeId|key`. */
  private pending = new Map<string, GizmoValue>();

  constructor(private readonly editor: LevelEditor) {}

  get armed(): string | null {
    return this.armedKey;
  }

  start() {
    const viz = this.editor.viz;
    this.ghosts = new GizmoGhosts(viz.overlayScene, {
      camera: viz.camera,
      canvas: viz.renderer.domElement,
      isDraggingGizmo: () => this.editor.transformControls?.gizmo.isDragging() ?? false,
    });
    viz.registerBeforeRenderCb(this.tick);
    this.onSelectionChanged();
  }

  stop() {
    this.editor.viz.unregisterBeforeRenderCb(this.tick);
    this.ghosts?.dispose();
    this.ghosts = null;
    this.current = null;
    this.rows = [];
    this.armedKey = null;
    this.armedContext = null;
  }

  private tick = () => {
    if (!this.ghosts) return;
    this.ghosts.syncGhosts(this.current ? this.ghostSpecs() : []);
    this.ghosts.update();
  };

  private bumpVersion() {
    this.editor.selection.state.gizmosVersion++;
  }

  /** Recompute the active target from the current selection; silently drops any armed handle. */
  onSelectionChanged() {
    const prevNode = this.current?.node ?? null;
    this.current = this.computeCurrent();
    this.rows = this.current ? this.buildRows(this.current) : [];
    if (this.armedKey !== null && this.current?.node !== prevNode) {
      // The caller re-attaches the object gizmo as part of selection sync.
      this.armedKey = null;
      this.armedContext = null;
    }
    this.bumpVersion();
  }

  /** Undo/redo changed `def.inputs`; resync pending values so ghosts/readouts don't lag the rebuild. */
  onInputsChanged(node: LevelSceneNode) {
    const cur = this.current;
    if (!cur || cur.node !== node) return;
    for (const row of this.rows) {
      const gv = this.gizmoValueFromInput(cur.def.inputs?.[row.key], row.gz);
      const pk = `${node.id}|${row.key}`;
      if (gv) this.pending.set(pk, gv);
      else this.pending.delete(pk);
    }
    this.bumpVersion();
  }

  /** A param rebuild landed: retained run data is fresh, pending values are now baked in. */
  onAssetDataRefreshed(node: LevelSceneNode) {
    for (const k of [...this.pending.keys()]) {
      if (k.startsWith(`${node.id}|`)) this.pending.delete(k);
    }
    if (this.current?.node !== node) return;
    this.current = this.computeCurrent();
    this.rows = this.current ? this.buildRows(this.current) : [];
    if (this.armedKey !== null && !(this.editor.transformControls?.gizmo.isDragging() ?? false)) {
      // Re-arm against the fresh row so origins/values track the new run.
      const key = this.armedKey;
      if (this.rows.some(r => r.key === key)) this.arm(key);
      else this.disarm();
    }
    this.bumpVersion();
  }

  interceptClick(raycaster: THREE.Raycaster): boolean {
    if (!this.ghosts || !this.current) return false;
    const hit = this.ghosts.pickGhost(raycaster);
    if (hit) {
      this.arm(hit.handleId); // GhostSpec.handleId carries the row key
      return true;
    }
    if (this.armedKey !== null) {
      this.disarm();
      return true;
    }
    return false;
  }

  disarmIfArmed(): boolean {
    if (this.armedKey === null) return false;
    this.disarm();
    return true;
  }

  arm(key: string) {
    const cur = this.current;
    const row = this.rows.find(r => r.key === key);
    const th = this.editor.transformControls;
    if (!cur || !row || !th) return;
    this.armedKey = key;
    this.armedContext = this.buildContext(row);
    if (row.kind === 'vec3') th.setMode('translate');

    const nodeId = cur.tree ? (cur.moduleToNodeId[row.module] ?? row.module) : row.module;
    th.attachTarget(
      new HandleTarget(
        nodeId,
        row.handleId,
        () => this.armedContext,
        () => this.current?.tree ?? null,
        {
          getBaseMatrix: () => this.current?.node.object.matrixWorld ?? null,
          getStoredValue: () => {
            const r = this.rows.find(rr => rr.key === key);
            return r && this.current ? this.currentGizmoValue(r) : undefined;
          },
          onChange: (phase, _nodeId, _handleId, value) => this.onHandleChange(key, phase, value),
        }
      ),
      row.gz.axes
    );
    this.bumpVersion();
  }

  disarm() {
    if (this.armedKey === null) return;
    this.armedKey = null;
    this.armedContext = null;
    this.editor.resyncSelection(); // re-attaches the object gizmo
    this.bumpVersion();
  }

  resetOverride(key: string) {
    const cur = this.current;
    if (!cur || cur.def.inputs?.[key] === undefined) return;
    this.pending.delete(`${cur.node.id}|${key}`);
    this.editor.setObjectInput(key, undefined);
    this.bumpVersion();
  }

  panelRows(): GizmoHandleRowInfo[] | null {
    if (!this.current || this.rows.length === 0) return null;
    const multiModule = new Set(this.rows.map(r => r.module)).size > 1;
    return this.rows.map(row => {
      const v = this.currentGizmoValue(row);
      return {
        key: row.key,
        module: multiModule ? row.module : null,
        handleId: row.handleId,
        kind: row.kind,
        color: row.color,
        readout: this.formatValue(row, v),
        overridden: this.current!.def.inputs?.[row.key] !== undefined,
        armed: row.key === this.armedKey,
      };
    });
  }

  private computeCurrent(): ActiveTarget | null {
    const editor = this.editor;
    const ctx = editor.resolveSelectedAsset();
    if (!ctx) return null;
    const gizmos =
      editor.assetGizmos.get(editor.effectiveAssetId(ctx.def)) ?? editor.assetGizmos.get(ctx.def.asset!);
    if (!gizmos || gizmos.length === 0) return null;
    return { node: ctx.node, def: ctx.def, gizmos, tree: ctx.tree, moduleToNodeId: ctx.moduleToNodeId };
  }

  private buildRows(cur: ActiveTarget): HandleRow[] {
    const controls =
      this.editor.assetControls.get(this.editor.effectiveAssetId(cur.def)) ??
      this.editor.assetControls.get(cur.def.asset!) ??
      [];
    return cur.gizmos.map((gz, i) => {
      const module = gz.sourceModule ?? '_root';
      const collides =
        cur.gizmos.some(o => o !== gz && o.handleId === gz.handleId) ||
        controls.some(c => c.handleId === gz.handleId);
      return {
        key: collides ? `${module}/${gz.handleId}` : gz.handleId,
        module,
        handleId: gz.handleId,
        kind: gz.kind,
        color: gizmoColorForIndex(i),
        gz,
      };
    });
  }

  private currentGizmoValue(row: HandleRow): GizmoValue {
    const pending = this.current ? this.pending.get(`${this.current.node.id}|${row.key}`) : undefined;
    if (pending) return pending;
    const gz = row.gz;
    if (gz.kind === 'transform') {
      return {
        kind: 'transform',
        mode: 'absolute',
        value: decomposeTransform3(_mat.fromArray(gz.value)),
      };
    }
    return {
      kind: 'vec3',
      mode: gz.absolute ? 'absolute' : 'delta',
      value: [gz.value[0], gz.value[1], gz.value[2]],
    };
  }

  private gizmoValueFromInput(v: InputValueJson | undefined, gz: RenderedGizmo): GizmoValue | null {
    if (!v) return null;
    if (v.type === 'vec3') {
      return { kind: 'vec3', mode: gz.absolute ? 'absolute' : 'delta', value: [...v.value] };
    }
    if (v.type === 'transform')
      return { kind: 'transform', mode: 'absolute', value: structuredClone(v.value) };
    return null;
  }

  private buildContext(row: HandleRow): HandleContext {
    const gz = row.gz;
    return {
      kind: gz.kind,
      mode: gz.kind === 'transform' || gz.absolute ? 'absolute' : 'delta',
      origin: [gz.origin[0], gz.origin[1], gz.origin[2]],
      transform:
        gz.kind === 'transform' && gz.value.length === 16
          ? decomposeTransform3(_mat.fromArray(gz.value))
          : undefined,
      axes: gz.axes,
    };
  }

  private onHandleChange(key: string, phase: 'preview' | 'commit', value: GizmoValue) {
    const cur = this.current;
    if (!cur) return;
    this.pending.set(`${cur.node.id}|${key}`, value);
    if (phase === 'commit') this.editor.setObjectInput(key, this.inputFromGizmoValue(value));
    this.bumpVersion();
  }

  private inputFromGizmoValue(v: GizmoValue): InputValueJson {
    if (v.kind === 'vec3') return { type: 'vec3', value: roundVec3(v.value as [number, number, number]) };
    const t = v.value as Transform3;
    return {
      type: 'transform',
      value: { pos: roundVec3(t.pos), rot: roundVec3(t.rot), scale: roundVec3(t.scale) },
    };
  }

  /** World transform of the module's declaring node (placement world × ancestor instance-0 chain). */
  private nodeWorldFor(row: HandleRow, out: THREE.Matrix4): THREE.Matrix4 {
    const cur = this.current!;
    out.copy(cur.node.object.matrixWorld);
    if (cur.tree) {
      const nodeId = cur.moduleToNodeId[row.module];
      if (nodeId) composeInstance0World(cur.tree, nodeId, out, _mat2);
    }
    return out;
  }

  private ghostSpecs(): GhostSpec[] {
    const specs: GhostSpec[] = [];
    for (const row of this.rows) {
      if (row.key === this.armedKey || row.gz.ghost === false) continue;
      const v = this.currentGizmoValue(row);
      let lp: readonly number[];
      if (row.kind === 'transform') {
        lp = (v.value as Transform3).pos;
      } else {
        const val = v.value as [number, number, number];
        lp =
          v.mode === 'absolute'
            ? val
            : [row.gz.origin[0] + val[0], row.gz.origin[1] + val[1], row.gz.origin[2] + val[2]];
      }
      _pos.set(lp[0], lp[1], lp[2]).applyMatrix4(this.nodeWorldFor(row, _mat));
      specs.push({
        handleId: row.key,
        kind: row.kind,
        color: row.color,
        position: [_pos.x, _pos.y, _pos.z],
      });
    }
    return specs;
  }

  private formatValue(row: HandleRow, v: GizmoValue): string {
    if (v.kind === 'transform') {
      const t = v.value as Transform3;
      return `⟨${t.pos.map(fmt).join(', ')}⟩`;
    }
    const a = v.value as [number, number, number];
    const parts = row.gz.axes.flatMap((on, i) => (on ? [fmt(a[i])] : []));
    return `⟨${parts.join(', ')}⟩`;
  }
}
