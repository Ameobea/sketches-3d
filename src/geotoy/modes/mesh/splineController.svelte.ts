import type * as THREE from 'three';
import { untrack } from 'svelte';

import type { Viz } from 'src/viz';
import { controlKey, splineControlPoints, type SplinePanelCtx } from 'src/geoscript/controlsUi';
import type { RenderedControl } from 'src/geoscript/runner/types';
import { SplineOverlay, type SplinePoint } from 'src/viz/gizmos/splineOverlay';
import type { TransformGizmo } from 'src/geotoy/modes/mesh/transformGizmo';
import type { TreeState } from 'src/geotoy/modules/treeState.svelte';

interface SplineControllerDeps {
  viz: Viz;
  treeState: TreeState;
  getGizmo: () => TransformGizmo | null;
  getModuleNameToNodeId: () => Record<string, string> | undefined;
  nodeWorldMatrix: (nodeId: string) => THREE.Matrix4;
  setGizmoTranslateMode: () => void;
  /** Suppress the selection's default arm while the spline owns the gizmo. */
  armNone: () => void;
  runOrFast: () => void;
}

/**
 * Spline-control viewport editing (`input_spline`): one control editable at a time; the
 * shared `SplineOverlay` owns markers/polyline/point-gizmo, this owns persistence + the
 * panel bridge. Reactive bits live in a `$state` object so the panel's getter reads track
 * through the deep proxy.
 */
export class SplineController {
  private readonly deps: SplineControllerDeps;
  private readonly state = $state({
    activeKey: null as string | null,
    points: [] as SplinePoint[],
    selectedIx: null as number | null,
  });
  private edit: { key: string; nodeId: string; handleId: string } | null = null;
  private overlay: SplineOverlay | null = null;
  private tick: (() => void) | null = null;

  readonly panelCtx: SplinePanelCtx;

  constructor(deps: SplineControllerDeps) {
    this.deps = deps;
    const state = this.state;
    this.panelCtx = {
      get activeKey() {
        return state.activeKey;
      },
      get points() {
        return state.points;
      },
      get selectedIx() {
        return state.selectedIx;
      },
      toggle: c => {
        if (this.edit?.key === controlKey(c)) this.exit();
        else this.enter(c);
      },
      select: ix => this.overlay?.selectPoint(ix),
      setPoint: (ix, p) => this.overlay?.setPoint(ix, p),
      add: () => this.overlay?.addPointAfter(),
      remove: ix => this.overlay?.deletePoint(ix),
    };

    // Exit spline editing when the selection moves off the owning node.
    $effect(() => {
      const sel = deps.treeState.state.selectedId;
      if (this.edit && sel !== this.edit.nodeId) untrack(this.exit);
    });
  }

  get activeKey() {
    return this.state.activeKey;
  }

  interceptClick = (raycaster: THREE.Raycaster): boolean => this.overlay?.interceptClick(raycaster) ?? false;

  exit = () => {
    if (!this.edit) return;
    this.edit = null;
    this.state.activeKey = null;
    this.state.selectedIx = null;
    if (this.tick) {
      this.deps.viz.unregisterBeforeRenderCb(this.tick);
      this.tick = null;
    }
    this.overlay?.dispose();
    this.overlay = null;
  };

  enter = (c: RenderedControl) => {
    this.exit();
    const { deps } = this;
    const nodeId = c.sourceModule ? deps.getModuleNameToNodeId()?.[c.sourceModule] : undefined;
    const g = deps.getGizmo();
    if (!nodeId || !g) return;
    deps.treeState.setSelected(nodeId);
    this.edit = { key: controlKey(c), nodeId, handleId: c.handleId };
    this.state.activeKey = this.edit.key;
    deps.armNone();
    const overlay = new SplineOverlay({
      overlayScene: deps.viz.overlayScene,
      camera: deps.viz.camera,
      canvas: deps.viz.renderer.domElement,
      getBaseMatrix: out => out.copy(deps.nodeWorldMatrix(nodeId)),
      attachGizmo: target => {
        deps.setGizmoTranslateMode();
        g.setCustomTarget(target);
      },
      detachGizmo: () => g.setCustomTarget(null),
      isDraggingGizmo: () => g.dragging(),
      onChange: (points, phase) => {
        this.state.points = points;
        if (phase === 'commit') this.commit(nodeId, c.handleId, points);
      },
      onSelectionChange: ix => {
        this.state.selectedIx = ix;
      },
    });
    this.overlay = overlay;
    const pts = splineControlPoints(c);
    overlay.setPoints(pts);
    this.state.points = pts;
    this.tick = () => overlay.tick();
    deps.viz.registerBeforeRenderCb(this.tick);
  };

  private commit(nodeId: string, handleId: string, points: SplinePoint[]) {
    const { treeState } = this.deps;
    const before = treeState.captureControl(nodeId, handleId);
    treeState.setControl(nodeId, handleId, { kind: 'spline', value: points });
    treeState.recordControlChange(nodeId, handleId, before, treeState.captureControl(nodeId, handleId));
    this.deps.runOrFast();
  }

  /** Refresh the active editor from the run channel (or exit if its control vanished). */
  onRunConsumed = (controls: RenderedControl[]) => {
    if (!this.edit) return;
    const sc = controls.find(c => controlKey(c) === this.edit!.key);
    if (!sc || sc.kind !== 'spline') {
      this.exit();
    } else if (!(this.deps.getGizmo()?.dragging() ?? false)) {
      const pts = splineControlPoints(sc);
      this.overlay?.setPoints(pts);
      this.state.points = pts;
    }
  };
}
