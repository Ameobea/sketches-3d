import * as THREE from 'three';
import { untrack } from 'svelte';

import type { GizmoValue, Transform3 } from 'src/geoscript/geotoyAPIClient';
import type { GizmoEditorHooks, GizmoReadout } from 'src/geoscript/gizmoExtensions';
import { scanGizmoHandleOrder } from 'src/geoscript/gizmoScan';
import type { RenderedGizmo, RenderedObject } from 'src/geoscript/runner/types';
import { decomposeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
import type { Viz } from 'src/viz';
import { GizmoGhosts, type GhostSpec } from 'src/viz/gizmos/gizmoGhosts';
import { gizmoColorForIndex } from 'src/viz/gizmos/gizmoPalette';
import type { GizmoTargetRef } from 'src/viz/gizmos/gizmoTypes';
import { untilOrbitControls } from 'src/viz/scenes/geoscriptPlayground/cameraControls';
import { installRaycastSelect } from 'src/viz/scenes/geoscriptPlayground/raycastSelect';
import {
  TransformGizmo,
  type GizmoMode,
  type GizmoSpace,
} from 'src/viz/scenes/geoscriptPlayground/transformGizmo';
import {
  buildParentMap,
  composeInstance0World,
  findParentId,
} from 'src/viz/scenes/geoscriptPlayground/treeOps';
import { GLOBALS_SELECTION_ID, type TreeState } from 'src/viz/scenes/geoscriptPlayground/treeState.svelte';

interface GizmoControllerDeps {
  viz: Viz;
  treeState: TreeState;
  renderMode: () => boolean;
  bootSignal: AbortSignal;
  /** Last run's reported gizmos; also the run-completed change token the sync/ghost effects key on. */
  getLastGizmos: () => RenderedGizmo[] | undefined;
  getRenderedObjects: () => RenderedObject[];
  runOrFast: () => void;
  blurEditor: () => void;
  isSplineActive: () => boolean;
  interceptSplineClick: (raycaster: THREE.Raycaster) => boolean;
}

// gizmo2d/gizmo1d store a full vec3 but expose only their active axes; project so the
// inline readout shows the right component count.
const projectAxes = (value: number[], axes: [boolean, boolean, boolean]): number[] => {
  const out: number[] = [];
  for (let i = 0; i < 3; i += 1) if (axes[i]) out.push(value[i] ?? 0);
  return out;
};

const channelReadout = (gz: RenderedGizmo): GizmoReadout =>
  gz.kind === 'transform'
    ? { kind: 'transform', transform: { pos: gz.origin, rot: [0, 0, 0], scale: [1, 1, 1] } }
    : { kind: 'vec3', values: projectAxes(gz.value, gz.axes) };

const storedReadout = (v: GizmoValue, axes: [boolean, boolean, boolean]): GizmoReadout =>
  v.kind === 'transform'
    ? { kind: 'transform', transform: v.value as GizmoReadout['transform'] }
    : { kind: 'vec3', values: projectAxes(v.value as number[], axes) };

const _ghostWorld = new THREE.Matrix4();
const _ghostScratch = new THREE.Matrix4();
const _ghostPos = new THREE.Vector3();

/**
 * Owns the viewport editing surface: the transform gizmo (+ arming model deciding what it
 * edits), gizmo ghosts, raycast selection, and the per-node inline readouts. Mounted async
 * once orbit controls exist; effects keep the gizmo/ghosts synced to selection + runs.
 */
export class GizmoController {
  private readonly deps: GizmoControllerDeps;

  gizmo = $state<TransformGizmo | null>(null);
  mode = $state<GizmoMode>('translate');
  space = $state<GizmoSpace>('local');
  showGhosts = $state(localStorage.getItem('geoscript-gizmo-ghosts') !== 'false');

  // What the viewport gizmo edits: an explicit arm (inspector / viewport click / chip)
  // recorded against the selection it was made under, falling back to the selected
  // node's first instance. A stale override (selection moved on, armed node/instance
  // deleted) falls back automatically — no latch, no same-tick clobber window.
  private armedOverride = $state<{ sel: string | null; ref: GizmoTargetRef | null } | null>(null);
  readonly armedRef = $derived.by((): GizmoTargetRef | null => {
    const sel = this.deps.treeState.state.selectedId;
    if (this.armedOverride?.sel !== sel) return this.defaultArmFor(sel);
    const ref = this.armedOverride.ref;
    if (ref === null) return null;
    const node = this.deps.treeState.state.tree.nodes[ref.nodeId];
    if (!node) return this.defaultArmFor(sel);
    if (ref.kind === 'instance' && !node.instances.some(i => i.id === ref.instanceId)) {
      return this.defaultArmFor(sel);
    }
    return ref;
  });

  // Per-node readout map: last run's reported values, overridden by the locally-stored
  // (live-edited) handle value so a drag updates the inline readout before re-eval.
  readonly readouts = $derived.by((): Map<string, GizmoReadout> => {
    const map = new Map<string, GizmoReadout>();
    const nodeId = this.deps.treeState.state.selectedId;
    const node = nodeId ? this.deps.treeState.state.tree.nodes[nodeId] : null;
    if (!node) return map;
    const axesByHandle = new Map<string, [boolean, boolean, boolean]>();
    for (const gz of this.deps.getLastGizmos() ?? []) {
      if (gz.sourceModule !== node.name) continue;
      axesByHandle.set(gz.handleId, gz.axes);
      map.set(gz.handleId, channelReadout(gz));
    }
    if (node.handles) {
      for (const [id, v] of Object.entries(node.handles)) {
        map.set(id, storedReadout(v, axesByHandle.get(id) ?? [true, true, true]));
      }
    }
    return map;
  });

  private ghosts: GizmoGhosts | null = null;
  private ghostTick: (() => void) | null = null;
  private gizmoTick: (() => void) | null = null;
  private raycastDisposer: (() => void) | null = null;
  private dragStartTransform: Transform3 | null = null;
  private dragStartHandle: GizmoValue | null = null;
  /**
   * Set for the duration of an instance drag, where the tree structure is frozen, so the
   * transform-only fast path can skip the eval-hash recompute + parent-map rebuild every frame.
   */
  dragSession: { parentMap: Map<string, string> } | null = null;

  constructor(deps: GizmoControllerDeps) {
    this.deps = deps;

    // Keep the gizmo bound to whatever is armed; re-sync after each run (ancestor world
    // transforms refresh). Reading `armedRef`/lastGizmos subscribes the effect to both.
    // Suspended while spline editing owns the gizmo via a custom target (re-fires on exit).
    $effect(() => {
      void this.armedRef;
      void deps.getLastGizmos();
      if (deps.isSplineActive()) return;
      this.gizmo?.syncTo(this.armedRef, deps.treeState.state.tree);
    });

    // Rebuild ghosts on discrete changes only (selection / arm / setting / each run); the
    // deep tree reads inside happen untracked so a drag's transform churn doesn't re-fire this.
    $effect(() => {
      void deps.treeState.state.selectedId;
      void this.armedRef;
      void this.showGhosts;
      void deps.getLastGizmos();
      untrack(this.rebuildGhosts);
    });
  }

  isDragging = () => this.gizmo?.dragging() ?? false;

  setMode = (mode: GizmoMode) => {
    this.mode = mode;
    this.gizmo?.setMode(mode);
  };

  toggleSpace = () => {
    this.space = this.space === 'world' ? 'local' : 'world';
    this.gizmo?.setSpace(this.space);
  };

  toggleGhosts = () => {
    this.showGhosts = !this.showGhosts;
    localStorage['geoscript-gizmo-ghosts'] = this.showGhosts ? 'true' : 'false';
  };

  private defaultArmFor(sel: string | null): GizmoTargetRef | null {
    const tree = this.deps.treeState.state.tree;
    if (sel === null || sel === GLOBALS_SELECTION_ID || sel === tree.rootId) {
      return null;
    }
    const node = tree.nodes[sel];
    if (!node || node.instances.length === 0) return null;
    return { kind: 'instance', nodeId: sel, instanceId: node.instances[0].id };
  }

  /** Arm a specific instance without disturbing selection (inspector / viewport click). */
  armInstance = (nodeId: string, instanceId: string) => {
    this.armedOverride = {
      sel: this.deps.treeState.state.selectedId,
      ref: { kind: 'instance', nodeId, instanceId },
    };
  };

  /** Explicitly arm nothing for the current selection (overrides the default arm). */
  armNone = () => {
    this.armedOverride = { sel: this.deps.treeState.state.selectedId, ref: null };
  };

  // World matrix of a node's representative (instance-0) copy, root → node inclusive — same
  // anchor `HandleTarget` uses, so a ghost sits exactly where its armed gizmo would.
  nodeWorldMatrix = (nodeId: string): THREE.Matrix4 => {
    _ghostWorld.identity();
    composeInstance0World(this.deps.treeState.state.tree, nodeId, _ghostWorld, _ghostScratch);
    return _ghostWorld;
  };

  // Ghosts only for the selected node's gizmos, at their live-gizmo positions. The armed
  // handle's own ghost is hidden (the real gizmo draws there instead).
  private rebuildGhosts = () => {
    if (!this.ghosts) return;
    const { treeState, renderMode, getLastGizmos } = this.deps;
    const sel = treeState.state.selectedId;
    const node = sel && sel !== GLOBALS_SELECTION_ID ? treeState.state.tree.nodes[sel] : null;
    if (renderMode() || !node) {
      this.ghosts.setGhosts([]);
      return;
    }
    const order = scanGizmoHandleOrder(node.source);
    const armedRef = this.armedRef;
    const armedHandle = armedRef?.kind === 'handle' && armedRef.nodeId === sel ? armedRef.name : null;
    const world = this.nodeWorldMatrix(sel!);
    const specs: GhostSpec[] = [];
    for (const gz of getLastGizmos() ?? []) {
      if (gz.sourceModule !== node.name || gz.handleId === armedHandle) continue;
      if (!(gz.ghost ?? this.showGhosts)) continue;
      // transform handles report a 16-float matrix; its translation is `origin`.
      const lp =
        gz.kind === 'transform'
          ? gz.origin
          : gz.absolute
            ? gz.value
            : [gz.origin[0] + gz.value[0], gz.origin[1] + gz.value[1], gz.origin[2] + gz.value[2]];
      _ghostPos.set(lp[0], lp[1], lp[2]).applyMatrix4(world);
      const ix = order.indexOf(gz.handleId);
      specs.push({
        handleId: gz.handleId,
        kind: gz.kind,
        color: gizmoColorForIndex(ix >= 0 ? ix : specs.length),
        position: [_ghostPos.x, _ghostPos.y, _ghostPos.z],
      });
    }
    this.ghosts.setGhosts(specs);
  };

  readonly editorHooks: GizmoEditorHooks = {
    arm: (handleId, kind) => {
      const { treeState } = this.deps;
      const sel = treeState.state.selectedId;
      // Handles are valid on any real node, including `_root` (unlike instance arming).
      if (!sel || sel === GLOBALS_SELECTION_ID || !treeState.state.tree.nodes[sel]) return;
      this.armedOverride = { sel, ref: { kind: 'handle', nodeId: sel, name: handleId } };
      if (kind === 'vec3') this.setMode('translate');
      this.deps.blurEditor();
    },
    disarm: () => {
      if (this.armedRef?.kind === 'handle') this.armedOverride = null;
    },
    resetHandle: handleId => {
      const { treeState } = this.deps;
      const sel = treeState.state.selectedId;
      const before = sel ? treeState.captureHandle(sel, handleId) : null;
      if (!sel || before === null) return; // already at default
      treeState.deleteHandle(sel, handleId);
      treeState.recordHandleChange(sel, handleId, before, null);
      this.deps.runOrFast();
    },
    setHandleVec3: (handleId, value) => {
      const { treeState } = this.deps;
      const sel = treeState.state.selectedId;
      if (!sel || !treeState.state.tree.nodes[sel]) return;
      const before = treeState.captureHandle(sel, handleId);
      const after: GizmoValue = {
        kind: 'vec3',
        mode: treeState.state.tree.nodes[sel].handles?.[handleId]?.mode ?? 'delta',
        value,
      };
      treeState.setHandle(sel, handleId, after);
      treeState.recordHandleChange(sel, handleId, before, after);
      this.deps.runOrFast();
    },
    getArmedHandleId: () => (this.armedRef?.kind === 'handle' ? this.armedRef.name : null),
  };

  /** Async gizmo/ghost/raycast setup once orbit controls exist; returns the teardown. */
  mount = () => {
    const { deps } = this;
    const { viz, treeState } = deps;
    let cancelled = false;
    (async () => {
      const orbit = await untilOrbitControls(viz, deps.bootSignal).catch(() => null);
      if (!orbit || cancelled) return;
      const g = new TransformGizmo(
        viz.camera,
        viz.renderer.domElement,
        viz.overlayScene,
        () => treeState.state.tree,
        {
          onDraggingChanged: dragging => {
            orbit.enabled = !dragging;
          },
          onDragStart: ref => {
            if (ref.kind === 'handle') {
              this.dragStartHandle = treeState.captureHandle(ref.nodeId, ref.name);
              return;
            }
            if (ref.kind !== 'instance') return;
            this.dragStartTransform = treeState.captureInstanceTransform(ref.nodeId, ref.instanceId);
            this.dragSession = { parentMap: buildParentMap(treeState.state.tree) };
          },
          onTransformChange: (ref, transform) => {
            if (ref.kind !== 'instance') return;
            treeState.setInstanceTransform(ref.nodeId, ref.instanceId, transform);
            deps.runOrFast();
          },
          onHandleChange: (nodeId, handleId, value) => {
            // Store + live readout per drag-tick, but defer the (geometry-changing) re-eval
            // to drag end — per-tick re-runs aren't smooth enough to be worth it.
            treeState.setHandle(nodeId, handleId, value);
          },
          onDragEnd: ref => {
            if (ref.kind === 'handle') {
              const after = treeState.captureHandle(ref.nodeId, ref.name);
              treeState.recordHandleChange(ref.nodeId, ref.name, this.dragStartHandle, after);
              this.dragStartHandle = null;
              deps.runOrFast();
              return;
            }
            if (ref.kind !== 'instance') return;
            this.dragSession = null;
            const after = treeState.captureInstanceTransform(ref.nodeId, ref.instanceId);
            if (this.dragStartTransform && after) {
              treeState.recordInstanceTransformChange(
                ref.nodeId,
                ref.instanceId,
                this.dragStartTransform,
                after
              );
            }
            this.dragStartTransform = null;
            deps.runOrFast();
          },
        }
      );
      // Resolve a handle's origin/kind/mode from the last run's channel + stored value.
      g.setHandleContextResolver((nodeId, handleId) => {
        const node = treeState.state.tree.nodes[nodeId];
        if (!node) return null;
        const reported = deps
          .getLastGizmos()
          ?.find(gz => gz.sourceModule === node.name && gz.handleId === handleId);
        const stored = node.handles?.[handleId];
        const kind = reported?.kind ?? stored?.kind ?? 'vec3';
        return {
          kind,
          mode: reported ? (reported.absolute ? 'absolute' : 'delta') : (stored?.mode ?? 'delta'),
          origin: reported?.origin ?? [0, 0, 0],
          transform:
            kind === 'transform' && reported?.value.length === 16
              ? decomposeTransform3(new THREE.Matrix4().fromArray(reported.value))
              : undefined,
          axes: reported?.axes ?? [true, true, true],
        };
      });
      const tickGizmo = () => g.update();
      viz.registerBeforeRenderCb(tickGizmo);

      const gh = new GizmoGhosts(viz.overlayScene, {
        camera: viz.camera,
        canvas: viz.renderer.domElement,
        isDraggingGizmo: () => g.dragging(),
      });
      const tickGhosts = () => gh.update();
      viz.registerBeforeRenderCb(tickGhosts);

      const disposer = installRaycastSelect({
        canvas: viz.renderer.domElement,
        camera: viz.camera,
        getCandidates: () =>
          deps.getRenderedObjects().filter(o => o instanceof THREE.Mesh && !!o.userData.sourceNodeId),
        interceptClick: raycaster => {
          if (deps.interceptSplineClick(raycaster)) return true;
          const hit = gh.pickGhost(raycaster);
          if (!hit) return false;
          this.editorHooks.arm(hit.handleId, hit.kind);
          return true;
        },
        onSelect: (id, instancePath) => {
          if (id === null) {
            // Background click: deselect to root (whose default arm is none).
            treeState.setSelected(treeState.state.tree.rootId);
            this.armedOverride = null;
            return;
          }
          const tree = treeState.state.tree;
          const node = tree.nodes[id];
          if (!node || id === tree.rootId) {
            treeState.setSelected(id);
            return;
          }
          // The clicked copy's own instance is the last element of its instance path.
          const clickedIdx = instancePath?.at(-1) ?? 0;
          const armId = (node.instances[clickedIdx] ?? node.instances[0]).id;
          // Multi-instance: select the parent so the inspector surfaces this node's
          // instance list (with the clicked instance armed); single-instance: select
          // the node itself, as before.
          treeState.setSelected(node.instances.length > 1 ? (findParentId(tree, id) ?? tree.rootId) : id);
          this.armInstance(id, armId);
        },
        isDraggingGizmo: () => g.dragging(),
      });
      if (cancelled) {
        viz.unregisterBeforeRenderCb(tickGizmo);
        viz.unregisterBeforeRenderCb(tickGhosts);
        disposer();
        gh.dispose();
        g.dispose();
        return;
      }
      this.gizmo = g;
      this.ghosts = gh;
      this.ghostTick = tickGhosts;
      this.raycastDisposer = disposer;
      this.gizmoTick = tickGizmo;
      this.rebuildGhosts();
    })();
    return () => {
      cancelled = true;
      this.raycastDisposer?.();
      this.raycastDisposer = null;
      if (this.gizmoTick) this.deps.viz.unregisterBeforeRenderCb(this.gizmoTick);
      this.gizmoTick = null;
      if (this.ghostTick) this.deps.viz.unregisterBeforeRenderCb(this.ghostTick);
      this.ghostTick = null;
      this.ghosts?.dispose();
      this.ghosts = null;
      this.gizmo?.dispose();
      this.gizmo = null;
    };
  };
}
