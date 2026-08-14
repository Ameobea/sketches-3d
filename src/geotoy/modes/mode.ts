// The editing environment for a tab, selected by the active tab's `kind` — there is no
// separate mode state, the mode *is* the selected tree.
//
// Deliberately small: only operations the shell performs generically live here. Anything
// mesh-specific (wireframe, projection, light helpers, material runtime) stays on
// `MeshScene` and is reached through the mode's own menu once the menubar exists.

import type { MeshTabView, TreeDef, TreeKind } from 'src/geoscript/geotoyAPIClient';
import type { GizmoEditorHooks } from 'src/geoscript/gizmoExtensions';
import type { RunResult } from 'src/geoscript/runner/runner';
import type { RunStats } from 'src/geoscript/runner/runner';

/** One cell of the run-status readout: `short` for the collapsed line, `label: value`
 *  for the expanded breakdown. */
export interface StatusMetric {
  label: string;
  value: string;
  short: string;
}

/** Mode-independent, so every mode reports runtime identically. */
export const runtimeMetric = (stats: RunStats): StatusMetric => {
  const ms = `${stats.runtimeMs.toFixed(2)} ms`;
  return { label: 'Runtime', value: ms, short: ms };
};

export interface MenuItem {
  label: string;
  /** Right-aligned shortcut column. */
  shortcut?: string;
  /** Current value shown inline, for items that toggle rather than fire (`on` / `off`). */
  state?: string;
  disabled?: boolean;
  action: () => void;
}

export interface MenuSection {
  header?: string;
  items: MenuItem[];
}

/**
 * Shell-owned actions a mode may place in its `scene` menu. The mode decides *what*
 * appears; the shell supplies *how* — same split as `statusMetrics(stats)`.
 */
export interface SceneMenuActions {
  openMaterialEditor: () => void;
  openEnvironment: () => void;
  exportScene: () => void;
  toggleRecording: () => void;
  recordingState: 'recording' | 'initializing' | 'not-recording';
}

/** Shell/controller-owned actions a mode may place in its `view` menu sections. */
export interface ViewMenuActions {
  toggleAxisHelpers: () => void;
  toggleProjection: () => void;
  toggleGizmoGhosts: () => void;
  showGizmoGhosts: boolean;
  gizmosExist: boolean;
}

export interface Mode {
  readonly kind: TreeKind;
  /** Apply a settled run to this mode's preview. */
  consume(result: RunResult, tree: TreeDef, moduleNameToNodeId: Record<string, string>): void;
  /** Drop everything the last run produced (cancel, unmount, tab switch). */
  clearScene(): void;
  /** Frame the selected subtree; `null` frames the whole output. */
  focus(nodeId: string | null): void;
  /** Per-tab view captured on switch; `null` for modes with nothing to restore. */
  buildViewState(): MeshTabView | null;
  /** `null` when the tab has no saved view — the mode supplies its own default. */
  restoreViewState(view: MeshTabView | null): void;
  /** What the run-status readout shows — mesh and texture report different things. */
  statusMetrics(stats: RunStats): StatusMetric[];
  /** Contents of the menubar's mode-owned `scene` menu. */
  sceneMenu(actions: SceneMenuActions): MenuSection[];
  /**
   * Mode-specific sections of the `view` menu (display/camera). The shell appends its own
   * `panels` section, which is mode-agnostic — so a mode that has no viewport contributes
   * nothing rather than showing controls that quietly do nothing.
   */
  viewSections(actions: ViewMenuActions): MenuSection[];
  /** Editor gizmo affordances; absent for modes that have none. */
  readonly editorHooks?: GizmoEditorHooks;
}
