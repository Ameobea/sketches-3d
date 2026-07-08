import * as THREE from 'three';

import { controlKey, splineControlPoints, type SplinePanelCtx } from 'src/geoscript/controlsUi';
import type { RenderedControl } from 'src/geoscript/runner/types';
import { SplineOverlay, type SplinePoint } from 'src/viz/gizmos/splineOverlay';
import { composeInstance0World } from 'src/viz/scenes/geoscriptPlayground/treeOps';

import type { EditorMode } from './editorMode';
import type { LevelEditor } from './LevelEditor.svelte';
import type { LevelSceneNode } from './levelSceneTypes';
import { round } from './mathUtils';

const _scratchMat = new THREE.Matrix4();

/**
 * Viewport editing of an `input_spline` control on the selected placement. Entered from the
 * inputs panel's spline row; owns the shared `SplineOverlay` and the transform gizmo (per
 * selected point), and commits edits as per-object `inputs` overrides through the normal
 * param-variant rebuild path.
 */
export class SplineEditMode implements EditorMode {
  readonly suppressSelectionHighlights = true;

  private readonly state = $state({
    activeKey: null as string | null,
    points: [] as SplinePoint[],
    selectedIx: null as number | null,
  });

  private overlay: SplineOverlay | null = null;
  private node: LevelSceneNode | null = null;
  private control: RenderedControl | null = null;

  readonly ctx: SplinePanelCtx;

  constructor(private readonly editor: LevelEditor) {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    this.ctx = {
      get activeKey() {
        return self.state.activeKey;
      },
      get points() {
        return self.state.points;
      },
      get selectedIx() {
        return self.state.selectedIx;
      },
      toggle: c => self.toggle(c),
      select: ix => self.overlay?.selectPoint(ix),
      setPoint: (ix, p) => self.overlay?.setPoint(ix, p),
      add: () => self.overlay?.addPointAfter(),
      remove: ix => self.overlay?.deletePoint(ix),
    };
  }

  private toggle(c: RenderedControl) {
    if (this.state.activeKey === controlKey(c)) this.exit();
    else this.enter(c);
  }

  private enter(c: RenderedControl) {
    this.exit();
    const editor = this.editor;
    const ctx = editor.resolveSelectedAsset();
    if (!ctx) return;
    const { node, tree, moduleToNodeId } = ctx;
    const viz = editor.viz;
    const module = c.sourceModule ?? '_root';

    this.node = node;
    this.control = c;
    this.state.activeKey = controlKey(c);
    editor.activeMode = this;

    this.overlay = new SplineOverlay({
      overlayScene: viz.overlayScene,
      camera: viz.camera,
      canvas: viz.renderer.domElement,
      getBaseMatrix: out => {
        out.copy(this.node!.object.matrixWorld);
        if (tree) {
          const treeNodeId = moduleToNodeId[module];
          if (treeNodeId) composeInstance0World(tree, treeNodeId, out, _scratchMat);
        }
        return out;
      },
      attachGizmo: target => {
        const th = editor.transformControls;
        if (!th) return;
        th.setMode('translate');
        th.attachTarget(target);
      },
      detachGizmo: () => editor.transformControls?.detach(),
      isDraggingGizmo: () => editor.transformControls?.gizmo.isDragging() ?? false,
      onChange: (points, phase) => {
        this.state.points = points;
        if (phase === 'commit') this.commit(points);
      },
      onSelectionChange: ix => {
        this.state.selectedIx = ix;
      },
    });
    viz.registerBeforeRenderCb(this.tick);

    const pts = this.currentPoints();
    this.overlay.setPoints(pts);
    this.state.points = pts;

    // The mode owns the gizmo (per-point); drop the object attachment + highlight.
    editor.transformControls?.detach();
    editor.updateSelectionState();
  }

  exit() {
    if (!this.node) return;
    const editor = this.editor;
    editor.viz.unregisterBeforeRenderCb(this.tick);
    this.overlay?.dispose();
    this.overlay = null;
    this.node = null;
    this.control = null;
    this.state.activeKey = null;
    this.state.selectedIx = null;
    this.state.points = [];
    if (editor.activeMode === this) editor.activeMode = null;
    editor.resyncSelection();
  }

  private tick = () => this.overlay?.tick();

  /** Override from `def.inputs`, else the retained run's reported points. */
  private currentPoints(): SplinePoint[] {
    const def = this.node ? this.editor.paramInputsDef(this.node) : null;
    const key = this.control?.handleId;
    const ov = key ? def?.inputs?.[key] : undefined;
    if (ov?.type === 'spline') return ov.value.map(p => [...p] as SplinePoint);
    return this.control ? splineControlPoints(this.control) : [];
  }

  private commit(points: SplinePoint[]) {
    const key = this.control?.handleId;
    if (!key) return;
    this.editor.setObjectInput(key, {
      type: 'spline',
      value: points.map(p => [round(p[0]), round(p[1]), round(p[2])] as SplinePoint),
    });
  }

  onKeyDown(e: KeyboardEvent): boolean {
    if (e.key === 'Delete') {
      if (this.state.selectedIx !== null) {
        e.preventDefault();
        this.overlay?.deletePoint();
      }
      return true;
    }
    if (e.key === 'Escape') {
      if (this.state.selectedIx !== null) this.overlay?.selectPoint(null);
      else this.exit();
      return true;
    }
    return false;
  }

  /** Consume every click while active: point picking only, never object re-selection. */
  interceptClick(raycaster: THREE.Raycaster, _event: PointerEvent): boolean {
    this.overlay?.interceptClick(raycaster);
    return true;
  }

  getFocusTarget(): THREE.Object3D | null {
    return this.node?.object ?? null;
  }

  onSelectNode(node: LevelSceneNode) {
    if (node !== this.node) this.exit();
  }

  onInputsChanged(node: LevelSceneNode) {
    if (node !== this.node) return;
    const pts = this.currentPoints();
    this.overlay?.setPoints(pts);
    if (!(this.editor.transformControls?.gizmo.isDragging() ?? false)) this.state.points = pts;
  }
}
