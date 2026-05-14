import * as THREE from 'three';

export interface ClickRaycasterOpts {
  canvas: HTMLCanvasElement;
  camera: THREE.Camera;
  /** Fires on confirmed clicks (left button, no drag). Raycaster is pre-positioned. */
  onClick: (ctx: { raycaster: THREE.Raycaster; event: PointerEvent }) => void;
  /** Return true to suppress the click (e.g. mid-gizmo-drag). */
  shouldIgnore?: () => boolean;
}

const DRAG_THRESHOLD_SQ = 16;

export const installClickRaycaster = (opts: ClickRaycasterOpts): (() => void) => {
  const raycaster = new THREE.Raycaster();
  const downPos = new THREE.Vector2();
  const ndc = new THREE.Vector2();
  let pointerMoved = false;

  const onPointerDown = (e: PointerEvent) => {
    downPos.set(e.clientX, e.clientY);
    pointerMoved = false;
  };

  const onPointerMove = (e: PointerEvent) => {
    if (pointerMoved) return;
    const dx = e.clientX - downPos.x;
    const dy = e.clientY - downPos.y;
    if (dx * dx + dy * dy > DRAG_THRESHOLD_SQ) pointerMoved = true;
  };

  const onPointerUp = (e: PointerEvent) => {
    if (pointerMoved || e.button !== 0) return;
    if (opts.shouldIgnore?.()) return;

    const rect = opts.canvas.getBoundingClientRect();
    ndc.set(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
    raycaster.setFromCamera(ndc, opts.camera);
    opts.onClick({ raycaster, event: e });
  };

  opts.canvas.addEventListener('pointerdown', onPointerDown);
  opts.canvas.addEventListener('pointermove', onPointerMove);
  opts.canvas.addEventListener('pointerup', onPointerUp);

  return () => {
    opts.canvas.removeEventListener('pointerdown', onPointerDown);
    opts.canvas.removeEventListener('pointermove', onPointerMove);
    opts.canvas.removeEventListener('pointerup', onPointerUp);
  };
};
