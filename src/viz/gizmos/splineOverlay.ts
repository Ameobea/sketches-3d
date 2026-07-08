import * as THREE from 'three';

import { buildAxisMaterial } from './gizmoMaterials';
import type { GizmoTarget, Transform3 } from './gizmoTypes';

export type SplinePoint = [number, number, number];

export interface SplineOverlayHost {
  overlayScene: THREE.Scene;
  camera: THREE.Camera;
  canvas: HTMLCanvasElement;
  /** Spline-local → world transform, resolved per frame so the overlay tracks its owner. */
  getBaseMatrix(out: THREE.Matrix4): THREE.Matrix4;
  /** Attach the host's shared transform gizmo (translate mode) to the selected point. */
  attachGizmo(target: GizmoTarget): void;
  detachGizmo(): void;
  isDraggingGizmo(): boolean;
  /** `preview`: per drag frame. `commit`: drag end, add/insert/delete, numeric edits. */
  onChange(points: SplinePoint[], phase: 'preview' | 'commit'): void;
  onSelectionChange?(ix: number | null): void;
}

const MARKER_SIZE = 0.014;
const SELECTED_SCALE = 1.35;
const ENDPOINT_SCALE = 1.25;
const SEGMENT_PICK_PX = 9;

// Subtle start→end tint so point ordering reads at a glance.
const COLOR_START = new THREE.Color('#22d3ee');
const COLOR_END = new THREE.Color('#a78bfa');
const colorAt = (t: number) => COLOR_START.clone().lerp(COLOR_END, t);

const markerGeometry = new THREE.SphereGeometry(1, 16, 12);

const _base = new THREE.Matrix4();
const _baseInv = new THREE.Matrix4();
const _v = new THREE.Vector3();
const _camPos = new THREE.Vector3();
const _ndc = new THREE.Vector2();
const _segA = new THREE.Vector3();
const _segB = new THREE.Vector3();

class SplinePointTarget implements GizmoTarget {
  constructor(private readonly overlay: SplineOverlay) {}

  private point(): SplinePoint | null {
    const ix = this.overlay.selectedIndex;
    return ix !== null ? (this.overlay.getPoints()[ix] ?? null) : null;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    const p = this.point();
    this.overlay.host.getBaseMatrix(out);
    if (!p) return out;
    return out.multiply(_scratchMat.makeTranslation(p[0], p[1], p[2]));
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    return this.overlay.host.getBaseMatrix(out);
  }

  getLocalTransform(out: Transform3): Transform3 {
    const p = this.point();
    out.pos[0] = p?.[0] ?? 0;
    out.pos[1] = p?.[1] ?? 0;
    out.pos[2] = p?.[2] ?? 0;
    out.rot[0] = out.rot[1] = out.rot[2] = 0;
    out.scale[0] = out.scale[1] = out.scale[2] = 1;
    return out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return 'YXZ';
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    this.overlay.moveSelectedPoint([t.pos[0], t.pos[1], t.pos[2]], phase);
  }
}

const _scratchMat = new THREE.Matrix4();

/**
 * Viewport editor for a sequence of vec3 control points (an `input_spline` value):
 * gradient-tinted point markers + a connecting polyline in the overlay scene. Click a
 * point to select it and grab the host's transform gizmo; double-click a segment to
 * insert; `addPointAfter`/`deletePoint` for structural edits. Points are in spline-local
 * space; the host's base matrix places them in the world. Shared by Geotoy and the
 * level editor — hosts only adapt gizmo attachment and value persistence.
 */
export class SplineOverlay {
  readonly host: SplineOverlayHost;
  private readonly group = new THREE.Group();
  private points: SplinePoint[] = [];
  private markers: THREE.Mesh[] = [];
  private hovered: THREE.Mesh | null = null;
  private _selected: number | null = null;
  private readonly raycaster = new THREE.Raycaster();
  private line: THREE.Line;
  private lineGeom = new THREE.BufferGeometry();
  private linePos = new THREE.BufferAttribute(new Float32Array(0), 3);

  constructor(host: SplineOverlayHost) {
    this.host = host;
    this.line = new THREE.Line(
      this.lineGeom,
      new THREE.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.75,
        premultipliedAlpha: true,
        depthTest: false,
        depthWrite: false,
      })
    );
    this.line.frustumCulled = false;
    this.line.renderOrder = -1;
    this.group.add(this.line);
    host.overlayScene.add(this.group);
    host.canvas.addEventListener('pointermove', this.onPointerMove);
    host.canvas.addEventListener('dblclick', this.onDblClick);
  }

  get selectedIndex(): number | null {
    return this._selected;
  }

  getPoints(): SplinePoint[] {
    return this.points.map(p => [...p] as SplinePoint);
  }

  /** Replace the point set (e.g. after undo or an external rebuild). Keeps selection when possible. */
  setPoints(points: SplinePoint[]): void {
    if (this.host.isDraggingGizmo()) return;
    this.points = points.map(p => [...p] as SplinePoint);
    if (this.markers.length !== this.points.length) this.rebuildMarkers();
    if (this._selected !== null && this._selected >= this.points.length) {
      this.selectPoint(this.points.length > 0 ? this.points.length - 1 : null);
    }
  }

  selectPoint(ix: number | null): void {
    this._selected = ix !== null && ix >= 0 && ix < this.points.length ? ix : null;
    if (this._selected !== null) {
      this.host.attachGizmo(new SplinePointTarget(this));
    } else {
      this.host.detachGizmo();
    }
    this.syncSelectionUniforms();
    this.host.onSelectionChange?.(this._selected);
  }

  /** Insert after `ix` (segment midpoint), or extend past the last point when at/omitting the end. */
  addPointAfter(ix?: number | null): void {
    const n = this.points.length;
    const at = ix ?? this._selected ?? n - 1;
    let p: SplinePoint;
    if (n === 0) {
      p = [0, 0, 0];
    } else if (at >= n - 1 || at < 0) {
      const last = this.points[n - 1];
      const prev = this.points[n - 2] ?? [last[0] - 2, last[1], last[2]];
      p = [last[0] * 2 - prev[0], last[1] * 2 - prev[1], last[2] * 2 - prev[2]];
    } else {
      const a = this.points[at];
      const b = this.points[at + 1];
      p = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2];
    }
    const insertIx = n === 0 ? 0 : Math.min(Math.max(at, 0), n - 1) + 1;
    this.points.splice(insertIx, 0, p);
    this.rebuildMarkers();
    this.selectPoint(insertIx);
    this.emit('commit');
  }

  deletePoint(ix: number | null = this._selected): void {
    if (ix === null || ix < 0 || ix >= this.points.length) return;
    this.points.splice(ix, 1);
    this.rebuildMarkers();
    this.selectPoint(this.points.length === 0 ? null : Math.min(ix, this.points.length - 1));
    this.emit('commit');
  }

  /** Numeric edit of one point (from a host panel field). */
  setPoint(ix: number, p: SplinePoint): void {
    if (ix < 0 || ix >= this.points.length) return;
    this.points[ix] = [...p];
    this.emit('commit');
  }

  moveSelectedPoint(p: SplinePoint, phase: 'preview' | 'commit'): void {
    if (this._selected === null) return;
    this.points[this._selected] = [...p];
    this.emit(phase);
  }

  /** Click routing from the host's pipeline. Consumes point clicks and armed-state empty clicks. */
  interceptClick(raycaster: THREE.Raycaster): boolean {
    const hit = this.pickPoint(raycaster);
    if (hit !== null) {
      this.selectPoint(hit);
      return true;
    }
    if (this._selected !== null) {
      this.selectPoint(null);
      return true;
    }
    return false;
  }

  /** Per-frame: track the base matrix, keep constant screen size, refresh the polyline. */
  tick(): void {
    const base = this.host.getBaseMatrix(_base);
    const cam = this.host.camera as THREE.PerspectiveCamera;
    _camPos.setFromMatrixPosition(cam.matrixWorld);
    const pos = this.linePos.array as Float32Array;

    for (let i = 0; i < this.markers.length; i++) {
      const m = this.markers[i];
      const p = this.points[i];
      m.position.set(p[0], p[1], p[2]).applyMatrix4(base);
      let s = MARKER_SIZE;
      if (cam.isPerspectiveCamera) {
        s = _camPos.distanceTo(m.position) * Math.tan((cam.fov * Math.PI) / 180 / 2) * MARKER_SIZE;
      }
      const isEndpoint = i === 0 || i === this.markers.length - 1;
      m.scale.setScalar(s * (i === this._selected ? SELECTED_SCALE : isEndpoint ? ENDPOINT_SCALE : 1));
      pos[i * 3] = m.position.x;
      pos[i * 3 + 1] = m.position.y;
      pos[i * 3 + 2] = m.position.z;
    }
    this.linePos.needsUpdate = true;
    this.line.visible = this.points.length > 1;
  }

  dispose(): void {
    this.host.canvas.removeEventListener('pointermove', this.onPointerMove);
    this.host.canvas.removeEventListener('dblclick', this.onDblClick);
    this.host.detachGizmo();
    for (const m of this.markers) (m.material as THREE.Material).dispose();
    (this.line.material as THREE.Material).dispose();
    this.lineGeom.dispose();
    this.host.overlayScene.remove(this.group);
  }

  private emit(phase: 'preview' | 'commit'): void {
    this.host.onChange(this.getPoints(), phase);
  }

  private rebuildMarkers(): void {
    for (const m of this.markers) {
      this.group.remove(m);
      (m.material as THREE.Material).dispose();
    }
    this.markers = [];
    this.hovered = null;
    const n = this.points.length;
    for (let i = 0; i < n; i++) {
      const mat = buildAxisMaterial({ color: colorAt(n > 1 ? i / (n - 1) : 0), shadeMin: 0.5 });
      const mesh = new THREE.Mesh(markerGeometry, mat);
      this.group.add(mesh);
      this.markers.push(mesh);
    }
    this.rebuildLine();
    this.syncSelectionUniforms();
  }

  // Resize the polyline buffers and bake the (count-only) vertex colors; positions
  // are filled per frame in `tick`.
  private rebuildLine(): void {
    const n = this.points.length;
    const colors = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) {
      const c = colorAt(n > 1 ? i / (n - 1) : 0);
      colors[i * 3] = c.r;
      colors[i * 3 + 1] = c.g;
      colors[i * 3 + 2] = c.b;
    }
    this.linePos = new THREE.BufferAttribute(new Float32Array(n * 3), 3);
    this.lineGeom.setAttribute('position', this.linePos);
    this.lineGeom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  }

  private syncSelectionUniforms(): void {
    this.markers.forEach((m, i) => {
      (m.material as THREE.ShaderMaterial).uniforms.uActive.value = i === this._selected ? 1 : 0;
    });
  }

  private pickPoint(raycaster: THREE.Raycaster): number | null {
    if (this.markers.length === 0) return null;
    const hits = raycaster.intersectObjects(this.markers, false);
    if (hits.length === 0) return null;
    const ix = this.markers.indexOf(hits[0].object as THREE.Mesh);
    return ix >= 0 ? ix : null;
  }

  /** Screen-space nearest segment within `SEGMENT_PICK_PX`; returns the insertion point (local). */
  private pickSegment(clientX: number, clientY: number): { after: number; point: SplinePoint } | null {
    if (this.points.length < 2) return null;
    const rect = this.host.canvas.getBoundingClientRect();
    const px = new THREE.Vector2(clientX - rect.left, clientY - rect.top);
    const base = this.host.getBaseMatrix(_base);
    const cam = this.host.camera;

    const toScreen = (p: SplinePoint, out: THREE.Vector2) => {
      _v.set(p[0], p[1], p[2]).applyMatrix4(base).project(cam);
      out.set(((_v.x + 1) / 2) * rect.width, ((1 - _v.y) / 2) * rect.height);
      return _v.z < 1;
    };

    const a2 = new THREE.Vector2();
    const b2 = new THREE.Vector2();
    const pt = new THREE.Vector2();
    let best: { after: number; distPx: number; t: number } | null = null;
    for (let i = 0; i < this.points.length - 1; i++) {
      if (!toScreen(this.points[i], a2) || !toScreen(this.points[i + 1], b2)) continue;
      const seg = pt.copy(b2).sub(a2);
      const lenSq = seg.lengthSq();
      const t = lenSq > 0 ? THREE.MathUtils.clamp(px.clone().sub(a2).dot(seg) / lenSq, 0, 1) : 0;
      const distPx = pt.copy(a2).addScaledVector(seg, t).distanceTo(px);
      if (distPx <= SEGMENT_PICK_PX && (!best || distPx < best.distPx)) best = { after: i, distPx, t };
    }
    if (!best) return null;

    // Insertion point: closest point on the world-space segment to the pick ray, back in local.
    _ndc.set((px.x / rect.width) * 2 - 1, -((px.y / rect.height) * 2 - 1));
    this.raycaster.setFromCamera(_ndc, cam);
    const pa = this.points[best.after];
    const pb = this.points[best.after + 1];
    _segA.set(pa[0], pa[1], pa[2]).applyMatrix4(base);
    _segB.set(pb[0], pb[1], pb[2]).applyMatrix4(base);
    const world = new THREE.Vector3();
    this.raycaster.ray.distanceSqToSegment(_segA, _segB, undefined, world);
    world.applyMatrix4(_baseInv.copy(base).invert());
    return { after: best.after, point: [world.x, world.y, world.z] };
  }

  private onDblClick = (e: MouseEvent) => {
    if (this.host.isDraggingGizmo()) return;
    const hit = this.pickSegment(e.clientX, e.clientY);
    if (!hit) return;
    this.points.splice(hit.after + 1, 0, hit.point);
    this.rebuildMarkers();
    this.selectPoint(hit.after + 1);
    this.emit('commit');
  };

  private onPointerMove = (e: PointerEvent) => {
    if (this.host.isDraggingGizmo() || this.markers.length === 0) {
      this.setHover(null);
      return;
    }
    const rect = this.host.canvas.getBoundingClientRect();
    _ndc.set(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(_ndc, this.host.camera);
    const hits = this.raycaster.intersectObjects(this.markers, false);
    this.setHover((hits[0]?.object as THREE.Mesh) ?? null);
  };

  private setHover(mesh: THREE.Mesh | null): void {
    if (mesh === this.hovered) return;
    if (this.hovered) (this.hovered.material as THREE.ShaderMaterial).uniforms.uHovered.value = 0;
    this.hovered = mesh;
    if (mesh) (mesh.material as THREE.ShaderMaterial).uniforms.uHovered.value = 1;
  }
}
