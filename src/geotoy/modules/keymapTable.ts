import type { GizmoMode } from 'src/geotoy/modes/mesh/transformGizmo';

const ROTATION_AMOUNT = Math.PI / 16;

export interface KeymapEntry {
  key: string;
  action: (event?: KeyboardEvent) => void;
  label: string;
  group?: string;
}

/** The shell-supplied action surface the keymap tables bind to. */
export interface GeotoyKeymapActions {
  run: () => void;
  toggleWireframe: () => void;
  toggleWireframeXray: () => void;
  toggleNormalMat: () => void;
  toggleLightHelpers: () => void;
  toggleAxesHelper: () => void;
  centerView: () => void;
  toggleProjection: () => void;
  snapView: (axis: 'x' | 'y' | 'z') => void;
  orbit: (axis: 'vertical' | 'horizontal', angle: number) => void;
  toggleRecording: () => void;
  setGizmoMode: (mode: GizmoMode) => void;
  toggleGizmoSpace: () => void;
  toggleSelectionSolo: () => void;
  escapeSelection: (event?: KeyboardEvent) => void;
  deleteSelected: () => void;
  startRenameSelected: () => void;
  treeUndo: (event?: KeyboardEvent) => void;
  treeRedo: (event?: KeyboardEvent) => void;
  toggleEditorCollapsed: () => void;
  togglePreview3d: () => void;
  /** Texture mode: camera keys only act while the 3D preview is showing. */
  preview3dActive: () => boolean;
}

type GetCtx = (() => GeotoyKeymapActions | null | undefined) | undefined;

/** Bindings that mean the same thing in every mode. */
export const buildCoreKeymap = (getCtx: GetCtx): KeymapEntry[] => [
  { key: 'ctrl+enter', action: () => getCtx?.()?.run(), label: 'run code' },
  {
    key: 'ctrl+e',
    action: e => {
      getCtx?.()?.toggleEditorCollapsed();
      e?.preventDefault();
    },
    label: 'show/hide editor panel',
  },
  { key: '.', action: () => getCtx?.()?.centerView(), label: 'center view on selection', group: 'camera' },
  { key: '/', label: 'solo selection', action: () => getCtx?.()?.toggleSelectionSolo(), group: 'selection' },
  {
    key: 'escape',
    label: 'unsolo or select root',
    action: e => getCtx?.()?.escapeSelection(e),
    group: 'selection',
  },
  {
    key: 'delete',
    label: 'delete selected node',
    action: () => getCtx?.()?.deleteSelected(),
    group: 'selection',
  },
  {
    key: 'f2',
    label: 'rename selected node',
    action: () => getCtx?.()?.startRenameSelected(),
    group: 'selection',
  },
  { key: 'ctrl+z', label: 'undo', action: e => getCtx?.()?.treeUndo(e), group: 'history' },
  { key: 'ctrl+y', label: 'redo', action: e => getCtx?.()?.treeRedo(e), group: 'history' },
  { key: 'ctrl+shift+z', label: 'redo', action: e => getCtx?.()?.treeRedo(e), group: 'history' },
];

/** Orbit-camera bindings; `enabled` gates them (texture mode: only with the 3D preview up). */
const cameraEntries = (getCtx: GetCtx, enabled: (ctx: GeotoyKeymapActions) => boolean): KeymapEntry[] => {
  const when = (f: (ctx: GeotoyKeymapActions) => void) => () => {
    const ctx = getCtx?.();
    if (ctx && enabled(ctx)) f(ctx);
  };
  return [
    {
      key: 'o',
      label: 'toggle perspective/orthographic',
      action: when(c => c.toggleProjection()),
      group: 'camera',
    },
    { key: '1', label: 'front/back view', action: when(c => c.snapView('z')), group: 'camera' },
    { key: '2', label: 'top/bottom view', action: when(c => c.snapView('y')), group: 'camera' },
    { key: '3', label: 'right/left view', action: when(c => c.snapView('x')), group: 'camera' },
    {
      key: 'arrowdown',
      label: 'orbit up',
      action: when(c => c.orbit('vertical', ROTATION_AMOUNT)),
      group: 'camera',
    },
    {
      key: 'arrowup',
      label: 'orbit down',
      action: when(c => c.orbit('vertical', -ROTATION_AMOUNT)),
      group: 'camera',
    },
    {
      key: 'arrowright',
      label: 'orbit right',
      action: when(c => c.orbit('horizontal', ROTATION_AMOUNT)),
      group: 'camera',
    },
    {
      key: 'arrowleft',
      label: 'orbit left',
      action: when(c => c.orbit('horizontal', -ROTATION_AMOUNT)),
      group: 'camera',
    },
  ];
};

export const buildMeshKeymap = (getCtx: GetCtx): KeymapEntry[] => [
  { key: 'w', action: () => getCtx?.()?.toggleWireframe(), label: 'toggle wireframe' },
  { key: 'shift+w', action: () => getCtx?.()?.toggleWireframeXray(), label: 'toggle wireframe x-ray' },
  { key: 'n', action: () => getCtx?.()?.toggleNormalMat(), label: 'toggle normal material' },
  { key: 'shift+l', action: () => getCtx?.()?.toggleLightHelpers(), label: 'toggle light helpers' },
  { key: 'a', action: () => getCtx?.()?.toggleAxesHelper(), label: 'toggle axes helper' },
  ...cameraEntries(getCtx, () => true),
  {
    key: 'g',
    label: 'translate gizmo',
    action: () => getCtx?.()?.setGizmoMode('translate'),
    group: 'selection',
  },
  { key: 'r', label: 'rotate gizmo', action: () => getCtx?.()?.setGizmoMode('rotate'), group: 'selection' },
  { key: 's', label: 'scale gizmo', action: () => getCtx?.()?.setGizmoMode('scale'), group: 'selection' },
  {
    key: 'l',
    label: 'toggle gizmo space (world/local)',
    action: () => getCtx?.()?.toggleGizmoSpace(),
    group: 'selection',
  },
  {
    key: 'ctrl+shift+p',
    label: 'start/stop recording',
    action: () => getCtx?.()?.toggleRecording(),
    group: 'recording',
  },
];

export const buildTextureKeymap = (getCtx: GetCtx): KeymapEntry[] => [
  { key: 'p', action: () => getCtx?.()?.togglePreview3d(), label: 'toggle 3d preview' },
  ...cameraEntries(getCtx, ctx => ctx.preview3dActive()),
];

/** Every binding across modes, for label-only listings (pause menu, docs). Keys shared by
 *  mode tables appear once. */
export const buildGeotoyKeymap = (getCtx?: GetCtx): KeymapEntry[] => {
  const seen = new Set<string>();
  return [...buildCoreKeymap(getCtx), ...buildMeshKeymap(getCtx), ...buildTextureKeymap(getCtx)].filter(e => {
    if (seen.has(e.key)) return false;
    seen.add(e.key);
    return true;
  });
};
