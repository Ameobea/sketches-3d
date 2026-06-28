import * as THREE from 'three';

import {
  projectAxisDrag,
  projectAxisScaleFactor,
  projectPlaneDrag,
  projectRotateDrag,
  projectUniformScale,
} from './dragMath';
import {
  buildArrowheadGeometry,
  buildArrowheadPickerGeometry,
  buildPlaneHandleGeometry,
  buildPlaneHandlePickerGeometry,
  buildRingDiscGeometry,
  buildRingPickerGeometry,
  buildShaftGeometry,
  buildShaftPickerGeometry,
  buildTaperedShaftGeometry,
  buildUniformScaleGeometry,
  buildUniformScalePickerGeometry,
} from './gizmoGeometry';
import {
  AXIS_COLORS,
  buildArrowheadMaterial,
  buildAxisMaterial,
  buildPickerMaterial,
  buildPlaneMaterial,
  buildRingMaterial,
} from './gizmoMaterials';
import {
  copyTransform3,
  makeTransform3,
  type GizmoHandleId,
  type GizmoMode,
  type GizmoSpace,
  type GizmoTarget,
  type Transform3,
} from './gizmoTypes';
import { Object3DTarget, type Object3DTargetCallbacks } from './targets';

export interface CustomGizmoCallbacks {
  onDragStart?(): void;
  onDrag?(): void;
  onDragEnd?(): void;
  onHoverChange?(hovered: GizmoHandleId | null): void;
}

interface Handle {
  id: GizmoHandleId;
  visual: THREE.Mesh;
  /** Fatter invisible mesh; gives raycast a forgiving hit target. */
  picker: THREE.Mesh;
  mode: GizmoMode;
}

interface DragState {
  handle: Handle;
  ndcStart: { x: number; y: number };
  pointerId: number;
  worldOrigin: THREE.Vector3;
  worldRotation: THREE.Quaternion;
  worldScale: THREE.Vector3;
  parentWorldInv: THREE.Matrix4;
  localStart: Transform3;
}

/**
 * Custom transform gizmo.  Caller adds it to `viz.overlayScene` and must
 * `update()` per frame so it tracks the target and keeps a constant on-screen size.
 */
export class CustomGizmo extends THREE.Object3D {
  private camera: THREE.Camera;
  private domElement: HTMLCanvasElement;
  private callbacks: CustomGizmoCallbacks;

  private target: GizmoTarget | null = null;
  private mode: GizmoMode = 'translate';
  private space: GizmoSpace = 'local';
  /** Restricts which translate handles are shown/pickable (for `gizmo2d`/`gizmo1d`). */
  private axisMask: [boolean, boolean, boolean] = [true, true, true];

  /** One root per mode; only the active mode's root is visible. */
  private translateRoot = new THREE.Group();
  private rotateRoot = new THREE.Group();
  private scaleRoot = new THREE.Group();
  private activePickers: THREE.Mesh[] = [];

  private handles: Handle[] = [];
  private handleByPicker = new Map<THREE.Mesh, Handle>();

  private dragState: DragState | null = null;
  private hovered: Handle | null = null;

  private _raycaster = new THREE.Raycaster();
  private _ndc = new THREE.Vector2();
  private _viewport = { width: 1, height: 1 };
  private _renderMatrix = new THREE.Matrix4();
  private _parentWorld = new THREE.Matrix4();
  private _scratchTransform = makeTransform3();

  private referenceSize = 1.0;
  sizeMultiplier = 1.0;

  constructor(camera: THREE.Camera, domElement: HTMLCanvasElement, callbacks: CustomGizmoCallbacks = {}) {
    super();
    this.camera = camera;
    this.domElement = domElement;
    this.callbacks = callbacks;
    // Without this, an untargeted gizmo flashes at scene origin before first attach.
    this.visible = false;

    this.add(this.translateRoot);
    this.add(this.rotateRoot);
    this.add(this.scaleRoot);

    this.buildTranslateHandles();
    this.buildRotateHandles();
    this.buildScaleHandles();

    this.applyModeVisibility();
    this.rebuildActivePickers();

    domElement.addEventListener('pointerdown', this.onPointerDown);
    domElement.addEventListener('pointermove', this.onPointerMove);
    domElement.addEventListener('pointerup', this.onPointerUp);
    domElement.addEventListener('pointercancel', this.onPointerUp);
  }

  dispose() {
    this.domElement.removeEventListener('pointerdown', this.onPointerDown);
    this.domElement.removeEventListener('pointermove', this.onPointerMove);
    this.domElement.removeEventListener('pointerup', this.onPointerUp);
    this.domElement.removeEventListener('pointercancel', this.onPointerUp);
    for (const h of this.handles) {
      h.visual.geometry.dispose();
      (h.visual.material as THREE.Material).dispose();
      h.picker.geometry.dispose();
      (h.picker.material as THREE.Material).dispose();
    }
    this.handles = [];
    this.handleByPicker.clear();
    if (this.parent) this.parent.remove(this);
  }

  setCamera(camera: THREE.Camera) {
    this.camera = camera;
  }

  setTarget(target: GizmoTarget | null) {
    if (this.dragState) return; // don't yank mid-drag
    this.target = target;
    this.visible = target !== null;
  }

  getTarget(): GizmoTarget | null {
    return this.target;
  }

  /** Named `bindObject` (not `attach`) so it doesn't shadow `Object3D.attach`, which reparents. */
  bindObject(obj: THREE.Object3D, callbacks: Object3DTargetCallbacks = {}) {
    this.setTarget(new Object3DTarget(obj, callbacks));
  }

  unbind() {
    this.setTarget(null);
  }

  getMode(): GizmoMode {
    return this.mode;
  }
  setMode(mode: GizmoMode) {
    if (this.mode === mode) return;
    this.mode = mode;
    this.applyModeVisibility();
    this.rebuildActivePickers();
    this.clearHover();
  }

  getSpace(): GizmoSpace {
    return this.space;
  }
  setSpace(space: GizmoSpace) {
    this.space = space;
  }

  isDragging(): boolean {
    return this.dragState !== null;
  }

  /** Call once per frame, before the overlay scene is rendered. */
  update() {
    if (!this.target) return;

    // Safe to re-track mid-drag: drag math is anchored to `dragState.worldOrigin`
    // captured at pointerdown, so deltas stay correct regardless of where we draw.
    this.target.getRenderMatrix(this._renderMatrix);
    const pos = new THREE.Vector3();
    const rot = new THREE.Quaternion();
    const scale = new THREE.Vector3();
    this._renderMatrix.decompose(pos, rot, scale);

    this.position.copy(pos);
    // Scale handles always use local axes (Blender convention).
    if (this.space === 'local' || this.mode === 'scale') {
      this.quaternion.copy(rot);
    } else {
      this.quaternion.identity();
    }
    this.scale.setScalar(this.computeAutoSize() * this.sizeMultiplier);
    this.updateMatrixWorld(true);
  }

  /** Uniform scale that keeps the gizmo at a roughly constant on-screen size. */
  private computeAutoSize(): number {
    if ((this.camera as THREE.PerspectiveCamera).isPerspectiveCamera) {
      const pcam = this.camera as THREE.PerspectiveCamera;
      const camPos = new THREE.Vector3().setFromMatrixPosition(pcam.matrixWorld);
      const dist = camPos.distanceTo(this.position);
      const fovRad = (pcam.fov * Math.PI) / 180;
      return this.referenceSize * (dist * Math.tan(fovRad / 2)) * 0.18;
    }
    if ((this.camera as THREE.OrthographicCamera).isOrthographicCamera) {
      const ocam = this.camera as THREE.OrthographicCamera;
      return this.referenceSize * ((ocam.top - ocam.bottom) / ocam.zoom) * 0.07;
    }
    return this.referenceSize;
  }

  private addHandle(
    mode: GizmoMode,
    parent: THREE.Group,
    id: GizmoHandleId,
    visualGeom: THREE.BufferGeometry,
    pickerGeom: THREE.BufferGeometry,
    material: THREE.ShaderMaterial,
    orient: (mesh: THREE.Object3D) => void,
    renderOrder = 1000
  ) {
    const visual = new THREE.Mesh(visualGeom, material);
    visual.renderOrder = renderOrder;
    orient(visual);
    parent.add(visual);

    const picker = new THREE.Mesh(pickerGeom, buildPickerMaterial());
    picker.userData.gizmoHandle = id;
    orient(picker);
    parent.add(picker);

    const handle: Handle = { id, visual, picker, mode };
    this.handles.push(handle);
    this.handleByPicker.set(picker, handle);
  }

  private buildTranslateHandles() {
    // Shaft/cone are modeled along +Y; rotate to align with target axis.
    const orientAxis = (axis: 'x' | 'y' | 'z') => (obj: THREE.Object3D) => {
      if (axis === 'x') obj.rotation.set(0, 0, -Math.PI / 2);
      else if (axis === 'z') obj.rotation.set(Math.PI / 2, 0, 0);
    };
    // Shaft tapers into the arrowhead so the line stays inside its silhouette at the tip.
    const translateShaftLength = 1.0;
    const translateShaftTaper = translateShaftLength * 0.05;
    for (const axis of ['x', 'y', 'z'] as const) {
      const color = AXIS_COLORS[axis];
      this.addHandle(
        'translate',
        this.translateRoot,
        { kind: 'translate-axis', axis },
        buildTaperedShaftGeometry({ length: translateShaftLength, taperLength: translateShaftTaper }),
        buildShaftPickerGeometry({ length: translateShaftLength }),
        buildAxisMaterial({ color }),
        orientAxis(axis)
      );
      this.addHandle(
        'translate',
        this.translateRoot,
        { kind: 'translate-axis', axis },
        buildArrowheadGeometry(),
        buildArrowheadPickerGeometry(),
        buildArrowheadMaterial({ color }),
        (obj: THREE.Object3D) => {
          orientAxis(axis)(obj);
          const tip = new THREE.Vector3(0, 0, 0);
          if (axis === 'x') tip.x = 0.85;
          else if (axis === 'y') tip.y = 0.85;
          else tip.z = 0.85;
          obj.position.copy(tip);
        },
        // Force draw-after-shaft; otherwise camera-distance sort flips and the
        // white outline sparkles where the two meet.
        1001
      );
    }

    const planeOffset = 0.32;
    const planes: Array<{
      axes: ['x', 'y'] | ['x', 'z'] | ['y', 'z'];
      color: number;
      orient: (o: THREE.Object3D) => void;
    }> = [
      {
        axes: ['x', 'y'],
        color: AXIS_COLORS.z, // plane handles take their normal axis's color
        orient: o => {
          o.position.set(planeOffset, planeOffset, 0);
        },
      },
      {
        axes: ['x', 'z'],
        color: AXIS_COLORS.y,
        orient: o => {
          o.position.set(planeOffset, 0, planeOffset);
          o.rotation.x = -Math.PI / 2;
        },
      },
      {
        axes: ['y', 'z'],
        color: AXIS_COLORS.x,
        orient: o => {
          o.position.set(0, planeOffset, planeOffset);
          o.rotation.y = Math.PI / 2;
        },
      },
    ];
    for (const p of planes) {
      this.addHandle(
        'translate',
        this.translateRoot,
        { kind: 'translate-plane', axes: p.axes },
        buildPlaneHandleGeometry(),
        buildPlaneHandlePickerGeometry(),
        buildPlaneMaterial({ color: p.color }),
        p.orient
      );
    }
  }

  private buildRotateHandles() {
    const ringOuter = 1.0;
    const ringInner = 0.92;
    // Disc is built in local XY (normal = +Z); rotate to put the normal on each axis.
    const orientations: Record<'x' | 'y' | 'z', (o: THREE.Object3D) => void> = {
      z: _o => {},
      x: o => o.rotation.set(0, Math.PI / 2, 0),
      y: o => o.rotation.set(-Math.PI / 2, 0, 0),
    };
    for (const axis of ['x', 'y', 'z'] as const) {
      this.addHandle(
        'rotate',
        this.rotateRoot,
        { kind: 'rotate-axis', axis },
        buildRingDiscGeometry({ outerRadius: ringOuter }),
        // tube is a radius, so 0.65 × band-width ≈ 1.3 × diameter — picker
        // fattens visible band by ~30%, no more.
        buildRingPickerGeometry({
          radius: (ringOuter + ringInner) / 2,
          tube: (ringOuter - ringInner) * 0.65,
        }),
        buildRingMaterial({
          color: AXIS_COLORS[axis],
          innerRadius: ringInner,
          outerRadius: ringOuter,
        }),
        orientations[axis]
      );
    }
  }

  private buildScaleHandles() {
    const orientAxis = (axis: 'x' | 'y' | 'z') => (obj: THREE.Object3D) => {
      if (axis === 'x') obj.rotation.set(0, 0, -Math.PI / 2);
      else if (axis === 'z') obj.rotation.set(Math.PI / 2, 0, 0);
    };
    for (const axis of ['x', 'y', 'z'] as const) {
      const color = AXIS_COLORS[axis];
      this.addHandle(
        'scale',
        this.scaleRoot,
        { kind: 'scale-axis', axis },
        buildShaftGeometry(),
        buildShaftPickerGeometry(),
        buildAxisMaterial({ color }),
        orientAxis(axis)
      );
      // Cube tip — distinct silhouette from translate's arrowhead.
      this.addHandle(
        'scale',
        this.scaleRoot,
        { kind: 'scale-axis', axis },
        new THREE.BoxGeometry(0.1, 0.1, 0.1),
        new THREE.BoxGeometry(0.18, 0.18, 0.18),
        buildAxisMaterial({ color, shadeMin: 0.4, edgeOutline: true }),
        (obj: THREE.Object3D) => {
          orientAxis(axis)(obj);
          const tip = new THREE.Vector3(0, 0, 0);
          if (axis === 'x') tip.x = 0.9;
          else if (axis === 'y') tip.y = 0.9;
          else tip.z = 0.9;
          obj.position.copy(tip);
        }
      );
    }

    const planeOffset = 0.32;
    const planes: Array<{
      axes: ['x', 'y'] | ['x', 'z'] | ['y', 'z'];
      color: number;
      orient: (o: THREE.Object3D) => void;
    }> = [
      { axes: ['x', 'y'], color: AXIS_COLORS.z, orient: o => o.position.set(planeOffset, planeOffset, 0) },
      {
        axes: ['x', 'z'],
        color: AXIS_COLORS.y,
        orient: o => {
          o.position.set(planeOffset, 0, planeOffset);
          o.rotation.x = -Math.PI / 2;
        },
      },
      {
        axes: ['y', 'z'],
        color: AXIS_COLORS.x,
        orient: o => {
          o.position.set(0, planeOffset, planeOffset);
          o.rotation.y = Math.PI / 2;
        },
      },
    ];
    for (const p of planes) {
      this.addHandle(
        'scale',
        this.scaleRoot,
        { kind: 'scale-plane', axes: p.axes },
        buildPlaneHandleGeometry(),
        buildPlaneHandlePickerGeometry(),
        buildPlaneMaterial({ color: p.color }),
        p.orient
      );
    }

    this.addHandle(
      'scale',
      this.scaleRoot,
      { kind: 'scale-uniform' },
      buildUniformScaleGeometry(),
      buildUniformScalePickerGeometry(),
      buildAxisMaterial({ color: 0xdddddd, shadeMin: 0.4, edgeOutline: true }),
      () => {}
    );
  }

  /** Only translate handles respect the mask; rotate/scale (transform handles) ignore it. */
  private passesMask(h: Handle): boolean {
    if (h.mode !== 'translate') return true;
    if (h.id.kind === 'translate-axis') return this.axisMask[this.axisIndex(h.id.axis)];
    if (h.id.kind === 'translate-plane') return h.id.axes.every(a => this.axisMask[this.axisIndex(a)]);
    return true;
  }

  private applyModeVisibility() {
    this.translateRoot.visible = this.mode === 'translate';
    this.rotateRoot.visible = this.mode === 'rotate';
    this.scaleRoot.visible = this.mode === 'scale';
    for (const h of this.handles) {
      if (h.mode === 'translate') h.visual.visible = this.passesMask(h);
    }
  }

  private rebuildActivePickers() {
    this.activePickers = this.handles
      .filter(h => h.mode === this.mode && this.passesMask(h))
      .map(h => h.picker);
  }

  setAxisMask(mask: [boolean, boolean, boolean]) {
    if (mask[0] === this.axisMask[0] && mask[1] === this.axisMask[1] && mask[2] === this.axisMask[2]) {
      return;
    }
    this.axisMask = mask;
    this.applyModeVisibility();
    this.rebuildActivePickers();
    this.clearHover();
  }

  private clearHover() {
    if (this.hovered) {
      (this.hovered.visual.material as THREE.ShaderMaterial).uniforms.uHovered.value = 0;
      this.hovered = null;
      this.callbacks.onHoverChange?.(null);
    }
  }

  private setHover(handle: Handle | null) {
    if (handle === this.hovered) return;
    if (this.hovered) (this.hovered.visual.material as THREE.ShaderMaterial).uniforms.uHovered.value = 0;
    this.hovered = handle;
    if (handle) (handle.visual.material as THREE.ShaderMaterial).uniforms.uHovered.value = 1;
    this.callbacks.onHoverChange?.(handle?.id ?? null);
  }

  private setActiveDrag(activeId: GizmoHandleId | null) {
    for (const h of this.handles) {
      const mat = h.visual.material as THREE.ShaderMaterial;
      const u = mat.uniforms.uActive;
      if (!u) continue;
      u.value = activeId && isRelatedToDrag(h.id, activeId) ? 1 : 0;
    }
    // Active state supersedes hover; don't read both at once.
    if (activeId && this.hovered) {
      (this.hovered.visual.material as THREE.ShaderMaterial).uniforms.uHovered.value = 0;
    }
  }

  private setRaycaster(e: PointerEvent) {
    const rect = this.domElement.getBoundingClientRect();
    this._viewport.width = rect.width;
    this._viewport.height = rect.height;
    this._ndc.set(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
    this._raycaster.setFromCamera(this._ndc, this.camera);
  }

  private hitTestActive(): Handle | null {
    if (this.activePickers.length === 0) return null;
    const hits = this._raycaster.intersectObjects(this.activePickers, false);
    if (hits.length === 0) return null;
    return this.handleByPicker.get(hits[0].object as THREE.Mesh) ?? null;
  }

  private onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0 || !this.target || this.dragState) return;
    this.setRaycaster(e);
    const handle = this.hitTestActive();
    if (!handle) return;
    e.preventDefault();
    e.stopPropagation();

    this.target.getRenderMatrix(this._renderMatrix);
    this.target.getParentWorldMatrix(this._parentWorld);
    const worldOrigin = new THREE.Vector3();
    const worldRotation = new THREE.Quaternion();
    const worldScale = new THREE.Vector3();
    this._renderMatrix.decompose(worldOrigin, worldRotation, worldScale);

    const parentWorldInv = new THREE.Matrix4().copy(this._parentWorld).invert();

    const localStart = makeTransform3();
    this.target.getLocalTransform(localStart);

    this.dragState = {
      handle,
      ndcStart: { x: this._ndc.x, y: this._ndc.y },
      pointerId: e.pointerId,
      worldOrigin,
      worldRotation,
      worldScale,
      parentWorldInv,
      localStart,
    };
    try {
      this.domElement.setPointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    this.setActiveDrag(handle.id);
    this.callbacks.onDragStart?.();
  };

  private onPointerMove = (e: PointerEvent) => {
    this.setRaycaster(e);
    if (!this.dragState) {
      if (!this.target) return;
      const handle = this.hitTestActive();
      this.setHover(handle);
      return;
    }
    this.applyDrag(this._ndc.x, this._ndc.y);
  };

  private onPointerUp = (e: PointerEvent) => {
    if (!this.dragState || e.pointerId !== this.dragState.pointerId) return;
    // Re-derive from current ndc in case the last pointermove was dropped.
    this.setRaycaster(e);
    this.applyDrag(this._ndc.x, this._ndc.y, /* commit */ true);
    this.dragState = null;
    this.setActiveDrag(null);
    try {
      this.domElement.releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    this.callbacks.onDragEnd?.();
  };

  /** Called from `onPointerMove` (preview) and `onPointerUp` (commit). */
  private applyDrag(ndcX: number, ndcY: number, commit = false) {
    const ds = this.dragState;
    if (!ds || !this.target) return;
    const ndcNow = { x: ndcX, y: ndcY };

    const axes = this.resolveWorldAxes(ds, ds.handle.id);
    const newLocal = this._scratchTransform;
    copyTransform3(newLocal, ds.localStart);

    switch (ds.handle.id.kind) {
      case 'translate-axis': {
        const d = projectAxisDrag(this.camera, ds.worldOrigin, axes.primary, ds.ndcStart, ndcNow);
        if (d === null) return;
        const worldDelta = _tmpVec3a.copy(axes.primary).multiplyScalar(d);
        this.applyWorldTranslate(newLocal, ds, worldDelta);
        break;
      }
      case 'translate-plane': {
        const normal = axes.normal!;
        const delta = projectPlaneDrag(this.camera, ds.worldOrigin, normal, ds.ndcStart, ndcNow, _tmpVec3a);
        if (!delta) return;
        this.applyWorldTranslate(newLocal, ds, delta);
        break;
      }
      case 'rotate-axis': {
        const angle = projectRotateDrag(this.camera, ds.worldOrigin, axes.primary, ds.ndcStart, ndcNow);
        if (angle === null) return;
        this.applyWorldRotate(newLocal, ds, axes.primary, angle);
        break;
      }
      case 'scale-axis': {
        const f = projectAxisScaleFactor(this.camera, ds.worldOrigin, axes.primary, ds.ndcStart, ndcNow);
        if (f === null) return;
        const axisIdx = this.axisIndex(ds.handle.id.axis);
        newLocal.scale[axisIdx] = ds.localStart.scale[axisIdx] * f;
        break;
      }
      case 'scale-plane': {
        // Single uniform factor across both in-plane axes — not two independent ratios.
        const normal = axes.normal!;
        const delta = projectPlaneDrag(this.camera, ds.worldOrigin, normal, ds.ndcStart, ndcNow, _tmpVec3a);
        if (!delta) return;
        const ref = this.computeAutoSize() * this.sizeMultiplier;
        const radial = delta.length();
        const outward =
          delta.dot(_tmpVec3b.copy(axes.primary).add(axes.secondary!).normalize()) >= 0 ? 1 : -1;
        const factor = 1 + (outward * radial) / Math.max(ref, 1e-6);
        const idxA = this.axisIndex(ds.handle.id.axes[0]);
        const idxB = this.axisIndex(ds.handle.id.axes[1]);
        newLocal.scale[idxA] = ds.localStart.scale[idxA] * factor;
        newLocal.scale[idxB] = ds.localStart.scale[idxB] * factor;
        break;
      }
      case 'scale-uniform': {
        const f = projectUniformScale(this.camera, ds.worldOrigin, ds.ndcStart, ndcNow, this._viewport);
        newLocal.scale[0] = ds.localStart.scale[0] * f;
        newLocal.scale[1] = ds.localStart.scale[1] * f;
        newLocal.scale[2] = ds.localStart.scale[2] * f;
        break;
      }
    }

    this.target.applyLocalTransform(newLocal, commit ? 'commit' : 'preview');
    this.callbacks.onDrag?.();
  }

  private applyWorldTranslate(out: Transform3, ds: DragState, worldDelta: THREE.Vector3) {
    const newWorldPos = _tmpVec3c.copy(ds.worldOrigin).add(worldDelta);
    newWorldPos.applyMatrix4(ds.parentWorldInv);
    out.pos[0] = newWorldPos.x;
    out.pos[1] = newWorldPos.y;
    out.pos[2] = newWorldPos.z;
  }

  private applyWorldRotate(out: Transform3, ds: DragState, axisWorld: THREE.Vector3, angle: number) {
    const deltaQ = _tmpQuatA.setFromAxisAngle(axisWorld, angle);
    const newWorldQ = _tmpQuatB.copy(deltaQ).multiply(ds.worldRotation);
    const newWorldM = _tmpMat4A.compose(ds.worldOrigin, newWorldQ, ds.worldScale);
    const newLocalM = _tmpMat4B.copy(ds.parentWorldInv).multiply(newWorldM);
    newLocalM.decompose(_tmpVec3c, _tmpQuatC, _tmpVec3d);
    out.pos[0] = _tmpVec3c.x;
    out.pos[1] = _tmpVec3c.y;
    out.pos[2] = _tmpVec3c.z;
    // Mismatched Euler order silently corrupts the target's stored rotation.
    const order = this.target?.getEulerOrder() ?? 'XYZ';
    const e = _tmpEulerA.setFromQuaternion(_tmpQuatC, order);
    out.rot[0] = e.x;
    out.rot[1] = e.y;
    out.rot[2] = e.z;
    out.scale[0] = _tmpVec3d.x;
    out.scale[1] = _tmpVec3d.y;
    out.scale[2] = _tmpVec3d.z;
  }

  private resolveWorldAxes(
    ds: DragState,
    id: GizmoHandleId
  ): {
    primary: THREE.Vector3;
    secondary?: THREE.Vector3;
    normal?: THREE.Vector3;
  } {
    const useLocal =
      this.space === 'local' ||
      id.kind === 'scale-axis' ||
      id.kind === 'scale-plane' ||
      id.kind === 'scale-uniform';

    const x = _basisX.set(1, 0, 0);
    const y = _basisY.set(0, 1, 0);
    const z = _basisZ.set(0, 0, 1);
    if (useLocal) {
      x.applyQuaternion(ds.worldRotation);
      y.applyQuaternion(ds.worldRotation);
      z.applyQuaternion(ds.worldRotation);
    }

    const fromAxis = (a: 'x' | 'y' | 'z'): THREE.Vector3 => (a === 'x' ? x : a === 'y' ? y : z);

    switch (id.kind) {
      case 'translate-axis':
      case 'scale-axis':
      case 'rotate-axis':
        return { primary: fromAxis(id.axis).clone() };
      case 'translate-plane':
      case 'scale-plane': {
        const [a1, a2] = id.axes;
        const primary = fromAxis(a1).clone();
        const secondary = fromAxis(a2).clone();
        const normal = new THREE.Vector3().crossVectors(primary, secondary).normalize();
        return { primary, secondary, normal };
      }
      case 'scale-uniform':
        return { primary: new THREE.Vector3(1, 0, 0) }; // unused
    }
  }

  private axisIndex(a: 'x' | 'y' | 'z'): 0 | 1 | 2 {
    return a === 'x' ? 0 : a === 'y' ? 1 : 2;
  }
}

// Plane drags also light their two contributing axes; scale-uniform lights every scale handle.
const isRelatedToDrag = (handle: GizmoHandleId, dragging: GizmoHandleId): boolean => {
  switch (dragging.kind) {
    case 'translate-axis':
    case 'rotate-axis':
    case 'scale-axis':
      return handle.kind === dragging.kind && 'axis' in handle && handle.axis === dragging.axis;
    case 'translate-plane':
    case 'scale-plane': {
      const [a1, a2] = dragging.axes;
      if (handle.kind === dragging.kind && 'axes' in handle) {
        return handle.axes[0] === a1 && handle.axes[1] === a2;
      }
      const axisKind = dragging.kind === 'translate-plane' ? 'translate-axis' : 'scale-axis';
      return handle.kind === axisKind && 'axis' in handle && (handle.axis === a1 || handle.axis === a2);
    }
    case 'scale-uniform':
      return handle.kind === 'scale-axis' || handle.kind === 'scale-uniform' || handle.kind === 'scale-plane';
  }
};

const _tmpVec3a = new THREE.Vector3();
const _tmpVec3b = new THREE.Vector3();
const _tmpVec3c = new THREE.Vector3();
const _tmpVec3d = new THREE.Vector3();
const _tmpQuatA = new THREE.Quaternion();
const _tmpQuatB = new THREE.Quaternion();
const _tmpQuatC = new THREE.Quaternion();
const _tmpMat4A = new THREE.Matrix4();
const _tmpMat4B = new THREE.Matrix4();
const _tmpEulerA = new THREE.Euler();
const _basisX = new THREE.Vector3();
const _basisY = new THREE.Vector3();
const _basisZ = new THREE.Vector3();
