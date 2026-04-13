import * as THREE from 'three';

import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import type { CsgAssetDef, CsgTreeNode } from './types';
import type { LevelObject } from './loadLevelDef';
import { LEVEL_PLACEHOLDER_MAT } from './levelObjectUtils';
import { generateComplementCode, generateSubtreeCode } from './csgCodeGen';
import { isOpNode, getNodeAtPath, computeNodePolarities } from './csgTreeUtils';
import type { LevelEditor } from './LevelEditor.svelte';
import type { CsgResolveRuntime } from './csgResolveRuntime';

const CSG_NEGATIVE_MAT = new THREE.MeshBasicMaterial({ color: 0xff3333, transparent: true, opacity: 0.4 });
const CSG_SELECTED_MAT = new THREE.MeshBasicMaterial({ color: 0x00ffff, wireframe: true });
const CSG_NESTED_NEGATIVE_MAT = new THREE.MeshBasicMaterial({
  color: 0x9966ff,
  transparent: true,
  opacity: 0.35,
});
const CSG_PICK_MAT = new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthWrite: false });

/**
 * Manages the Three.js preview objects that visualise the CSG tree during
 * interactive editing.
 *
 * Owns:
 * - the preview object maps and selectable mesh registry
 * - render configuration selection (which nodes to show and how)
 * - subtree / complement preview building via the geoscript worker
 * - preview transform syncing for non-selected nodes during drags
 */
export class CsgPreviewScene {
  private editGroup: THREE.Group | null = null;
  private editLevelObj: LevelObject | null = null;
  private isActive = false;

  private nodePreviews = new Map<string, THREE.Object3D>();
  private resolvedPreviews = new Map<string, { wrapper: THREE.Group; preview: THREE.Object3D }>();
  private _nodePolarities = new Map<string, 'positive' | 'negative'>();
  private selectableMeshes: THREE.Mesh[] = [];
  private meshToNodePath = new Map<THREE.Mesh, string>();
  private complementPreview: THREE.Object3D | null = null;
  /** Incremented on every config change to invalidate in-flight async tasks. */
  private configGeneration = 0;

  constructor(
    private readonly editor: LevelEditor,
    private readonly runtime: CsgResolveRuntime
  ) {}

  /** Polarity map recomputed on each applyRenderConfig call. */
  get nodePolarities(): Map<string, 'positive' | 'negative'> {
    return this._nodePolarities;
  }

  // ---------------------------------------------------------------------------
  // Public API
  // ---------------------------------------------------------------------------

  /** Begin a CSG edit session for the given group and level object. */
  activate(editGroup: THREE.Group, editLevelObj: LevelObject): void {
    this.editGroup = editGroup;
    this.editLevelObj = editLevelObj;
    this.isActive = true;
  }

  /**
   * End the CSG edit session: mark inactive, tear down all preview objects,
   * and clear the group/level-object references.
   */
  deactivate(): void {
    this.isActive = false;
    this.teardown();
    this.editGroup = null;
    this.editLevelObj = null;
  }

  /**
   * Clear all preview objects and bump the config generation so in-flight
   * async resolves know to discard their results.
   */
  teardown(): void {
    this.configGeneration++;
    if (this.editGroup) {
      while (this.editGroup.children.length > 0) {
        this.editGroup.remove(this.editGroup.children[0]);
      }
    }
    this.nodePreviews.clear();
    this.resolvedPreviews.clear();
    this.selectableMeshes.length = 0;
    this.meshToNodePath.clear();
    this.complementPreview = null;
  }

  /**
   * Rebuild the full preview scene for the given selection state.
   * Recomputes node polarities from the current asset def, then fires async
   * subtree/complement resolves.
   *
   * Config 1 (no selection): full CSG result visible + negative subtree overlays.
   * Config ROOT (root selected): full result visible, gizmo on level object.
   * Config 2 (positive node selected): full result hidden, complement + selection.
   * Config 3 (negative node selected): full result visible, selection in red.
   */
  applyRenderConfig(selectedNodePath: string | null, assetName: string | null): void {
    this.teardown(); // bumps configGeneration and clears objects
    this.editor.transformControls?.detach();

    if (!this.editGroup || !this.editLevelObj || !assetName) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    this._nodePolarities = computeNodePolarities(csgDef.tree);

    if (selectedNodePath === null) {
      // Config 1: no selection — show full result with negative overlays
      this.editLevelObj.object.visible = true;
      const negPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, ''));
      for (const path of this.collectSubtreePaths(csgDef.tree, '')) {
        if (negPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
      for (const path of negPaths) {
        void this.resolveSubtreePreview(path, CSG_NEGATIVE_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
      return;
    }

    if (selectedNodePath === '') {
      // Config ROOT: root selected — show full result, gizmo on level object
      this.editLevelObj.object.visible = true;
      this.editor.transformControls?.attach(this.editLevelObj.object);
      for (const path of this.collectSubtreePaths(csgDef.tree, '')) {
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
      return;
    }

    const polarity = this.nodePolarities.get(selectedNodePath) ?? 'positive';

    if (polarity === 'positive') {
      // Config 2: positive selection — hide full result, show complement + selection
      this.editLevelObj.object.visible = false;
      void this.resolveComplementPreview(selectedNodePath, assetName);
      void this.resolveSubtreePreview(selectedNodePath, CSG_SELECTED_MAT, assetName, {
        attachGizmo: true,
        pickable: true,
        trackNodePreview: true,
      });
      const negPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, selectedNodePath));
      for (const path of this.collectSubtreePaths(csgDef.tree, selectedNodePath)) {
        if (negPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
      for (const path of negPaths) {
        void this.resolveSubtreePreview(path, CSG_NEGATIVE_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
    } else {
      // Config 3: negative selection — full result visible, overlay selection in red
      this.editLevelObj.object.visible = true;
      void this.resolveSubtreePreview(selectedNodePath, CSG_NEGATIVE_MAT, assetName, {
        attachGizmo: true,
        pickable: true,
        trackNodePreview: true,
      });
      const nestedNegPaths = new Set(this.collectNegativeSubtreePaths(csgDef.tree, selectedNodePath));
      for (const path of this.collectSubtreePaths(csgDef.tree, selectedNodePath)) {
        if (nestedNegPaths.has(path)) continue;
        void this.resolveSubtreePreview(path, CSG_PICK_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
      for (const path of nestedNegPaths) {
        void this.resolveSubtreePreview(path, CSG_NESTED_NEGATIVE_MAT, assetName, {
          pickable: true,
          trackNodePreview: false,
        });
      }
    }
  }

  /**
   * Raycast against CSG node previews and the root level object.
   * Returns the hit path string ('' for root), or `undefined` if nothing was hit.
   * The controller uses the return value to call selectNode / deselectNode.
   */
  pickNode(raycaster: THREE.Raycaster): string | undefined {
    const hits = raycaster.intersectObjects(this.selectableMeshes, false);
    if (hits.length > 0) {
      const path = this.meshToNodePath.get(hits[0].object as THREE.Mesh);
      if (path !== undefined) return path;
    }
    if (this.editLevelObj?.object.visible) {
      const rootHits = raycaster.intersectObject(this.editLevelObj.object, true);
      if (rootHits.length > 0) return '';
    }
    return undefined;
  }

  /** Return the tracked preview object for a node path (used for transform writeback). */
  getNodePreview(path: string): THREE.Object3D | null {
    return this.nodePreviews.get(path) ?? null;
  }

  /**
   * Sync the transforms of all non-selected resolved previews from the current
   * tree state. Called during drag (objectChange) to keep non-selected peers
   * in sync while the user moves the selected node.
   */
  syncResolvedPreviewTransforms(tree: CsgTreeNode, selectedNodePath: string | null, assetName: string): void {
    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    for (const [path, entry] of this.resolvedPreviews) {
      if (path === selectedNodePath) continue;
      const node = getNodeAtPath(tree, path);
      this.applyMatrixTransform(entry.wrapper, this.buildAncestorMatrix(csgDef, path));
      this.applyNodeTransform(entry.preview, node);
    }
  }

  // ---------------------------------------------------------------------------
  // Internal helpers
  // ---------------------------------------------------------------------------

  /** Collect paths of negative children of difference ops within a subtree. */
  private collectNegativeSubtreePaths(tree: CsgTreeNode, rootPath: string): string[] {
    const node = rootPath ? getNodeAtPath(tree, rootPath) : tree;
    const paths: string[] = [];
    const walk = (n: CsgTreeNode, path: string) => {
      if (!isOpNode(n)) return;
      for (let i = 0; i < n.children.length; i++) {
        const childPath = path ? `${path}.${i}` : `${i}`;
        if (n.op === 'difference' && i > 0) paths.push(childPath);
        walk(n.children[i], childPath);
      }
    };
    walk(node, rootPath);
    return paths;
  }

  /** Collect all descendant paths within a subtree, excluding the root. */
  private collectSubtreePaths(tree: CsgTreeNode, rootPath: string): string[] {
    const node = rootPath ? getNodeAtPath(tree, rootPath) : tree;
    const paths: string[] = [];
    const walk = (n: CsgTreeNode, path: string) => {
      if (path !== rootPath) paths.push(path);
      if (!isOpNode(n)) return;
      for (let i = 0; i < n.children.length; i++) {
        const childPath = path ? `${path}.${i}` : `${i}`;
        walk(n.children[i], childPath);
      }
    };
    walk(node, rootPath);
    return paths;
  }

  /** Build the cumulative ancestor transform matrix for a subtree path. */
  private buildAncestorMatrix(csgDef: CsgAssetDef, path: string): THREE.Matrix4 {
    const ancestorMatrix = new THREE.Matrix4();
    if (!path) return ancestorMatrix;

    const parts = path.split('.');
    for (let i = 0; i < parts.length; i++) {
      const ancestorPath = parts.slice(0, i).join('.');
      const ancestor = ancestorPath ? getNodeAtPath(csgDef.tree, ancestorPath) : csgDef.tree;
      if (isOpNode(ancestor)) {
        const [px = 0, py = 0, pz = 0] = ancestor.position ?? [];
        const [rx = 0, ry = 0, rz = 0] = ancestor.rotation ?? [];
        const [sx = 1, sy = 1, sz = 1] = ancestor.scale ?? [];
        if (px || py || pz || rx || ry || rz || sx !== 1 || sy !== 1 || sz !== 1) {
          const m = new THREE.Matrix4();
          const euler = new THREE.Euler(rx, ry, rz, 'YXZ');
          m.compose(
            new THREE.Vector3(px, py, pz),
            new THREE.Quaternion().setFromEuler(euler),
            new THREE.Vector3(sx, sy, sz)
          );
          ancestorMatrix.multiply(m);
        }
      }
    }
    return ancestorMatrix;
  }

  private applyNodeTransform(
    object: THREE.Object3D,
    node: {
      position?: [number, number, number];
      rotation?: [number, number, number];
      scale?: [number, number, number];
    }
  ) {
    const [px = 0, py = 0, pz = 0] = node.position ?? [];
    const [rx = 0, ry = 0, rz = 0] = node.rotation ?? [];
    const [sx = 1, sy = 1, sz = 1] = node.scale ?? [];
    object.position.set(px, py, pz);
    object.rotation.set(rx, ry, rz, 'YXZ');
    object.scale.set(sx, sy, sz);
  }

  private applyMatrixTransform(object: THREE.Object3D, matrix: THREE.Matrix4) {
    const position = new THREE.Vector3();
    const quaternion = new THREE.Quaternion();
    const scale = new THREE.Vector3();
    matrix.decompose(position, quaternion, scale);
    object.position.copy(position);
    object.quaternion.copy(quaternion);
    object.scale.copy(scale);
  }

  /**
   * Resolve a subtree at `path` and add it as a preview mesh to the CSG edit group.
   * If `attachGizmo` is true, also attaches the transform controls to it.
   */
  private async resolveSubtreePreview(
    path: string,
    material: THREE.Material,
    assetName: string,
    options: { attachGizmo?: boolean; pickable?: boolean; trackNodePreview?: boolean } = {}
  ): Promise<void> {
    const attachGizmo = options.attachGizmo ?? false;
    const pickable = options.pickable ?? true;
    const trackNodePreview = options.trackNodePreview ?? attachGizmo;
    const generation = this.configGeneration;
    if (!this.editGroup) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    const { modules: subModules, code: subCode } = generateSubtreeCode(
      csgDef,
      path,
      this.editor.levelDef.assets
    );
    const modules = { ...subModules, code: subCode };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      result = await this.runtime.queuePreviewResolve(async () => {
        if (generation !== this.configGeneration || !this.isActive || !this.editGroup) return null;
        const { repl, ctxPtrPromise } = this.runtime.getPreviewRuntime();
        const ctxPtr = await ctxPtrPromise;
        return runGeoscript({ code: renderWrapper, ctxPtr, repl, includePrelude: false, modules });
      });
    } catch (error) {
      console.error(`[CsgPreviewScene] Subtree resolve failed for "${path}":`, error);
      this.runtime.terminatePreviewWorker();
      return;
    }

    if (!result) return;
    if (result.error) {
      console.error(`[CsgPreviewScene] Subtree resolve failed for "${path}":`, result.error);
      return;
    }
    if (generation !== this.configGeneration || !this.isActive || !this.editGroup) return;

    const meshes: THREE.Mesh[] = [];
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, material);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) return;

    const preview: THREE.Object3D =
      meshes.length === 1
        ? meshes[0]
        : (() => {
            const g = new THREE.Group();
            meshes.forEach(m => g.add(m));
            return g;
          })();

    // A wrapper group carries the ancestor transform so the preview's own
    // local transform represents only this node's contribution.
    const wrapper = new THREE.Group();
    wrapper.applyMatrix4(this.buildAncestorMatrix(csgDef, path));
    this.editGroup.add(wrapper);

    const node = getNodeAtPath(csgDef.tree, path);
    this.applyNodeTransform(preview, node);
    wrapper.add(preview);

    this.resolvedPreviews.set(path, { wrapper, preview });
    if (trackNodePreview) this.nodePreviews.set(path, preview);

    preview.traverse(child => {
      if (child instanceof THREE.Mesh && pickable) {
        this.selectableMeshes.push(child);
        this.meshToNodePath.set(child, path);
      }
    });

    if (attachGizmo) this.editor.transformControls?.attach(preview);
  }

  /**
   * Resolve the complement (full tree minus the selected subtree) and show it as
   * solid geometry. Used in Config 2 (positive node selected) to provide context.
   */
  private async resolveComplementPreview(excludePath: string, assetName: string): Promise<void> {
    const generation = this.configGeneration;
    if (!this.editGroup) return;

    const csgDef = this.editor.levelDef.assets[assetName] as CsgAssetDef;
    const complementResult = generateComplementCode(csgDef, excludePath, this.editor.levelDef.assets);
    if (!complementResult) return; // selected root — no complement

    const modules = { ...complementResult.modules, code: complementResult.code };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      result = await this.runtime.queuePreviewResolve(async () => {
        if (generation !== this.configGeneration || !this.isActive || !this.editGroup) return null;
        const { repl, ctxPtrPromise } = this.runtime.getPreviewRuntime();
        const ctxPtr = await ctxPtrPromise;
        return runGeoscript({ code: renderWrapper, ctxPtr, repl, includePrelude: false, modules });
      });
    } catch (error) {
      console.error(`[CsgPreviewScene] Complement resolve failed:`, error);
      this.runtime.terminatePreviewWorker();
      return;
    }

    if (!result) return;
    if (result.error) {
      console.error(`[CsgPreviewScene] Complement resolve failed:`, result.error);
      return;
    }
    if (generation !== this.configGeneration || !this.isActive || !this.editGroup) return;

    const levelMat = this.editLevelObj?.def.material
      ? (this.editor.builtMaterials.get(this.editLevelObj.def.material) ?? LEVEL_PLACEHOLDER_MAT)
      : LEVEL_PLACEHOLDER_MAT;

    const meshes: THREE.Mesh[] = [];
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, levelMat);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) return;

    if (this.complementPreview) this.editGroup.remove(this.complementPreview);

    const complement: THREE.Object3D =
      meshes.length === 1
        ? meshes[0]
        : (() => {
            const g = new THREE.Group();
            meshes.forEach(m => g.add(m));
            return g;
          })();

    this.complementPreview = complement;
    this.editGroup.add(complement);
    // Complement is purely visual context — not selectable
  }
}
