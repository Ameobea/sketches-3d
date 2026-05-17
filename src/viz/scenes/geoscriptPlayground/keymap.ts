import type { ReplCtx } from './types';

const ROTATION_AMOUNT = Math.PI / 16;

export interface KeymapEntry {
  key: string;
  action: (event?: KeyboardEvent) => void;
  label: string;
  group?: string;
}

export const buildGeotoyKeymap = (getCtx?: () => ReplCtx | null | undefined): KeymapEntry[] => [
  { key: 'w', action: () => getCtx?.()?.toggleWireframe(), label: 'toggle wireframe' },
  { key: 'shift+w', action: () => getCtx?.()?.toggleWireframeXray(), label: 'toggle wireframe x-ray' },
  { key: 'n', action: () => getCtx?.()?.toggleNormalMat(), label: 'toggle normal material' },
  { key: 'ctrl+enter', action: () => getCtx?.()?.run(), label: 'run code' },
  { key: 'shift+l', action: () => getCtx?.()?.toggleLightHelpers(), label: 'toggle light helpers' },
  { key: 'a', action: () => getCtx?.()?.toggleAxesHelper(), label: 'toggle axes helper' },

  { key: '.', action: () => getCtx?.()?.centerView(), label: 'center view on selection', group: 'camera' },
  { key: '1', label: 'front/back view', action: () => getCtx?.()?.snapView('z'), group: 'camera' },
  { key: '2', label: 'top/bottom view', action: () => getCtx?.()?.snapView('y'), group: 'camera' },
  { key: '3', label: 'right/left view', action: () => getCtx?.()?.snapView('x'), group: 'camera' },

  {
    key: 'arrowdown',
    label: 'orbit up',
    action: () => getCtx?.()?.orbit('vertical', ROTATION_AMOUNT),
    group: 'camera',
  },
  {
    key: 'arrowup',
    label: 'orbit down',
    action: () => getCtx?.()?.orbit('vertical', -ROTATION_AMOUNT),
    group: 'camera',
  },
  {
    key: 'arrowright',
    label: 'orbit right',
    action: () => getCtx?.()?.orbit('horizontal', ROTATION_AMOUNT),
    group: 'camera',
  },
  {
    key: 'arrowleft',
    label: 'orbit left',
    action: () => getCtx?.()?.orbit('horizontal', -ROTATION_AMOUNT),
    group: 'camera',
  },

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

  {
    key: 'ctrl+shift+p',
    label: 'start/stop recording',
    action: () => getCtx?.()?.toggleRecording(),
    group: 'recording',
  },
];
