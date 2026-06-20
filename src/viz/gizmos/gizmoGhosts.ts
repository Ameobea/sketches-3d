import * as THREE from 'three';

import { buildGhostMaterial } from './gizmoMaterials';

export interface GhostSpec {
  handleId: string;
  kind: 'vec3' | 'transform';
  /** Hex; the handle's categorical color (per-node scan order). */
  color: string;
  /** World-space position the live gizmo would occupy. */
  position: [number, number, number];
}

export interface GizmoGhostsOpts {
  camera: THREE.Camera;
  canvas: HTMLCanvasElement;
  isDraggingGizmo: () => boolean;
}

interface GhostMesh {
  mesh: THREE.Mesh;
  spec: GhostSpec;
}

// Square frustum (trapezoidal prism), faceted. Shared across all ghosts; scaled per-frame
// to keep a constant on-screen size like the live gizmo.
const ghostGeometry = (() => {
  const g = new THREE.CylinderGeometry(0.45, 0.8, 1.1, 4).toNonIndexed();
  g.computeVertexNormals();
  return g;
})();

const GHOST_SIZE = 0.025;
const _camPos = new THREE.Vector3();
const _ndc = new THREE.Vector2();

/** Non-interactive overlay markers for a tree node's `gizmo(...)` sites. Clicking one arms
 *  its handle (same as the editor badge); hovering ramps it to full saturation/opacity. */
export class GizmoGhosts {
  private readonly group = new THREE.Group();
  private readonly overlay: THREE.Scene;
  private readonly opts: GizmoGhostsOpts;
  private readonly raycaster = new THREE.Raycaster();
  private meshes: GhostMesh[] = [];
  private hovered: THREE.Mesh | null = null;

  constructor(overlayScene: THREE.Scene, opts: GizmoGhostsOpts) {
    this.overlay = overlayScene;
    this.opts = opts;
    overlayScene.add(this.group);
    opts.canvas.addEventListener('pointermove', this.onPointerMove);
  }

  setGhosts(specs: GhostSpec[]): void {
    for (const { mesh } of this.meshes) {
      this.group.remove(mesh);
      (mesh.material as THREE.Material).dispose();
    }
    this.meshes = [];
    this.hovered = null;
    for (const spec of specs) {
      const mesh = new THREE.Mesh(ghostGeometry, buildGhostMaterial(spec.color));
      mesh.position.set(spec.position[0], spec.position[1], spec.position[2]);
      this.group.add(mesh);
      this.meshes.push({ mesh, spec });
    }
  }

  /** Per-frame: keep a constant on-screen size. */
  update(): void {
    if (this.meshes.length === 0) return;
    const cam = this.opts.camera as THREE.PerspectiveCamera;
    _camPos.setFromMatrixPosition(cam.matrixWorld);
    for (const { mesh } of this.meshes) {
      let s = GHOST_SIZE;
      if (cam.isPerspectiveCamera) {
        const dist = _camPos.distanceTo(mesh.position);
        s = dist * Math.tan((cam.fov * Math.PI) / 180 / 2) * GHOST_SIZE;
      }
      mesh.scale.setScalar(s);
    }
  }

  /** For the click pipeline: resolve a pre-positioned raycaster to a ghost, if any. */
  pickGhost(raycaster: THREE.Raycaster): { handleId: string; kind: 'vec3' | 'transform' } | null {
    const hit = this.intersect(raycaster);
    return hit ? { handleId: hit.spec.handleId, kind: hit.spec.kind } : null;
  }

  private intersect(raycaster: THREE.Raycaster): GhostMesh | null {
    if (this.meshes.length === 0) return null;
    const hits = raycaster.intersectObjects(
      this.meshes.map(m => m.mesh),
      false
    );
    if (hits.length === 0) return null;
    return this.meshes.find(m => m.mesh === hits[0].object) ?? null;
  }

  private onPointerMove = (e: PointerEvent) => {
    if (this.opts.isDraggingGizmo() || this.meshes.length === 0) {
      this.setHover(null);
      return;
    }
    const rect = this.opts.canvas.getBoundingClientRect();
    _ndc.set(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(_ndc, this.opts.camera);
    this.setHover(this.intersect(this.raycaster)?.mesh ?? null);
  };

  private setHover(mesh: THREE.Mesh | null): void {
    if (mesh === this.hovered) return;
    if (this.hovered) (this.hovered.material as THREE.ShaderMaterial).uniforms.uHover.value = 0;
    this.hovered = mesh;
    if (mesh) (mesh.material as THREE.ShaderMaterial).uniforms.uHover.value = 1;
  }

  dispose(): void {
    this.opts.canvas.removeEventListener('pointermove', this.onPointerMove);
    this.setGhosts([]);
    this.overlay.remove(this.group);
  }
}
