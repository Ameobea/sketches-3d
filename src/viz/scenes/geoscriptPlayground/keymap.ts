import type { ReplCtx } from './types';

const ROTATION_AMOUNT = Math.PI / 16;

export interface KeymapEntry {
  key: string;
  action: () => void;
  label: string;
  group?: string;
}

export const buildGeotoyKeymap = (getCtx?: () => ReplCtx | null | undefined) => [
  { key: 'w', action: () => getCtx?.()?.toggleWireframe(), label: 'toggle wireframe' },
  { key: 'n', action: () => getCtx?.()?.toggleNormalMat(), label: 'toggle normal material' },
  { key: 'ctrl+enter', action: () => getCtx?.()?.run(), label: 'run code' },
  { key: 'l', action: () => getCtx?.()?.toggleLightHelpers(), label: 'toggle light helpers' },
  { key: 'a', action: () => getCtx?.()?.toggleAxesHelper(), label: 'toggle axes helper' },

  { key: '.', action: () => getCtx?.()?.centerView(), label: 'center view', group: 'camera' },
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
];
