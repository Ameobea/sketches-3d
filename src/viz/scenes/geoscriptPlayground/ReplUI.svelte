<script lang="ts">
  import * as THREE from 'three';
  import { onDestroy, onMount } from 'svelte';
  import { EditorView, type KeyBinding } from '@codemirror/view';
  import type * as Comlink from 'comlink';

  import type { Viz } from 'src/viz';
  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
  import { buildEditor } from '../../../geoscript/editor';
  import { buildAndAddLight } from './lights';
  import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
  import SaveControls from './SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { DefaultCameraPos, DefaultCameraTarget, type ReplCtx, type RunStats } from './types';
  import ReplOutput from './ReplOutput.svelte';
  import ReplControls from './ReplControls.svelte';
  import ExportModal from './ExportModal.svelte';
  import {
    buildMaterial,
    FallbackMat,
    HiddenMat,
    LineMat,
    NormalMat,
    WireframeMat,
    type MaterialDefinitions,
  } from 'src/geoscript/materials';
  import MaterialEditor from './materialEditor/MaterialEditor.svelte';
  import {
    getMultipleTextures,
    type CompositionVersionMetadata,
    type TextureID,
  } from 'src/geoscript/geotoyAPIClient';
  import { Textures } from './materialEditor/state.svelte';
  import {
    clearSavedState,
    getIsDirty,
    getServerState,
    getView,
    loadState,
    saveState,
    setLastRunWasSuccessful,
  } from './persistence';
  import { CustomShaderMaterial } from 'src/viz/shaders/customShader';
  import { CustomBasicShaderMaterial } from 'src/viz/shaders/customBasicShader';

  let {
    viz,
    geoscriptWorker: repl,
    setReplCtx,
    userData,
    onHeightChange,
  }: {
    viz: Viz;
    geoscriptWorker: Comlink.Remote<GeoscriptWorkerMethods>;
    setReplCtx: (ctx: ReplCtx) => void;
    userData?: GeoscriptPlaygroundUserData;
    onHeightChange: (height: number, isCollapsed: boolean) => void;
  } = $props();

  const {
    code: initialCode,
    materials: initialMatDefs,
    lastRunWasSuccessful,
    view: initialView,
  } = loadState(userData);

  let ctxPtr = $state<number | null>(null);

  let isDirty = $state(getIsDirty(userData));

  let innerWidth = $state(window.innerWidth);
  let isEditorCollapsed = $state(
    (() => {
      const raw = localStorage.getItem('geoscriptEditorCollapsed');
      return typeof raw === 'string' ? raw === 'true' : innerWidth < 768;
    })()
  );
  $effect(() => {
    localStorage.setItem('geoscriptEditorCollapsed', isEditorCollapsed ? 'true' : 'false');
  });
  $effect(() => {
    if (innerWidth >= 768 && isEditorCollapsed) {
      isEditorCollapsed = false;
      onHeightChange(height, isEditorCollapsed);
    }
  });

  let height = $state(
    Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight)
  );
  let lastCode = initialCode;

  onMount(() => {
    onHeightChange(height, isEditorCollapsed);

    repl.init().then(ptr => {
      ctxPtr = ptr;
    });
  });

  const handleMousedown = (e: MouseEvent) => {
    e.preventDefault();

    const handleMousemove = (e: MouseEvent) => {
      const newHeight = Math.min(window.innerHeight * 0.9, Math.max(100, window.innerHeight - e.clientY));
      height = newHeight;
      onHeightChange(height, isEditorCollapsed);
    };

    const handleMouseup = () => {
      window.removeEventListener('mousemove', handleMousemove);
      window.removeEventListener('mouseup', handleMouseup);
    };

    window.addEventListener('mousemove', handleMousemove);
    window.addEventListener('mouseup', handleMouseup);
  };

  let err: string | null = $state(null);
  let isRunning: boolean = $state(false);
  let runStats: RunStats | null = $state(null);
  const includePrelude = true;
  let renderedObjects: (
    | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
    | THREE.Line<THREE.BufferGeometry, THREE.Material>
    | THREE.Light
  )[] = $state([]);

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  let didFirstRun = $state(false);
  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    if (didFirstRun) {
      return;
    }
    didFirstRun = true;

    // if the user closed the tab while the last run was in progress, avoid eagerly running it again in
    // case there was an infinite loop or something
    if (lastRunWasSuccessful) {
      run(initialCode);
    }
  });

  const beforeUnloadHandler = () => {
    if (editorView) {
      saveState(
        {
          code: editorView.state.doc.toString(),
          materials: materialDefinitions,
          view: getView(viz),
        },
        userData
      );
    }
  };

  const setupEditor = () => {
    if (!codemirrorContainer) {
      if (editorView) {
        beforeUnloadHandler();
        editorView.destroy();
        editorView = null;
      }
      return;
    }

    if (editorView) {
      return;
    }

    const customKeymap: readonly KeyBinding[] = [
      {
        key: 'Ctrl-Enter',
        run: () => {
          if (!editorView) {
            return true;
          }
          run();
          return true;
        },
      },
      {
        key: 'Ctrl-.',
        run: () => {
          centerView();
          return true;
        },
      },
      {
        key: 'Ctrl-s',
        run: () => {
          if (editorView) {
            saveState(
              {
                code: editorView.state.doc.toString(),
                materials: materialDefinitions,
                view: getView(viz),
              },
              userData
            );
          }
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container: codemirrorContainer,
      customKeymap,
      initialCode: lastCode,
      onDocChange: () => {
        isDirty = true;
      },
    });
    editorView = editor.editorView;
  };

  onDestroy(() => {
    if (editorView) {
      beforeUnloadHandler();
      editorView.destroy();
    }
  });

  $effect(setupEditor);

  let materialOverride = $state<'wireframe' | 'normal' | null>(null);

  const fetchAndSetTextures = async (textureIDs: TextureID[]) => {
    if (textureIDs.length === 0) {
      return;
    }

    const adminToken = new URLSearchParams(window.location.search).get('admin_token') ?? undefined;
    await getMultipleTextures(textureIDs, undefined, adminToken).then(textures => {
      const allTextures = { ...Textures.textures };
      for (const texture of textures) {
        allTextures[texture.id] = texture;
      }
      Textures.textures = allTextures;
    });
  };

  const computeCompositeBoundingBox = (
    objects: (
      | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
      | THREE.Line<THREE.BufferGeometry, THREE.Material>
      | THREE.Light
    )[]
  ): THREE.Box3 => {
    const box = new THREE.Box3();
    for (const obj of objects) {
      if (!(obj instanceof THREE.Mesh || obj instanceof THREE.Line)) {
        continue;
      }

      obj.geometry.computeBoundingBox();
      const meshBox = obj.geometry.boundingBox;
      if (meshBox) box.union(meshBox.applyMatrix4(obj.matrixWorld));
    }
    return box;
  };

  const centerView = async () => {
    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    if (!renderedObjects.length) {
      viz.camera.position.copy(DefaultCameraPos);
      viz.orbitControls!.target.copy(DefaultCameraTarget);
      viz.camera.lookAt(DefaultCameraTarget);
      viz.orbitControls!.update();
      return;
    }

    const compositeBbox = computeCompositeBoundingBox(renderedObjects);
    const boundingSphere = new THREE.Sphere();
    compositeBbox.getBoundingSphere(boundingSphere);
    let center = boundingSphere.center;
    let radius = boundingSphere.radius;

    if (Number.isNaN(center.x) || Number.isNaN(center.y) || Number.isNaN(center.z)) {
      center = new THREE.Vector3(0, 0, 0);
    }
    if (radius <= 0 || Number.isNaN(radius)) {
      radius = 1;
    }

    // try to keep the same look direction
    const lookDir = new THREE.Vector3();
    lookDir.copy(viz.camera.position).sub(viz.orbitControls!.target);

    if (lookDir.lengthSq() === 0) {
      lookDir.set(1, 1, 1);
    }
    lookDir.normalize();

    const camera = viz.camera as THREE.PerspectiveCamera;
    let distance;

    if (!camera.isPerspectiveCamera) {
      console.warn('centerView only works with PerspectiveCamera, falling back to old method');
      const size = new THREE.Vector3();
      compositeBbox.getSize(size);
      const maxDim = Math.max(size.x, size.y, size.z);
      distance = maxDim * 1.2 + 1;
    } else {
      const vfov = THREE.MathUtils.degToRad(camera.fov);
      const hfov = 2 * Math.atan(Math.tan(vfov / 2) * camera.aspect);
      const fov = Math.min(vfov, hfov);

      // Compute distance to fit bounding sphere in view
      distance = radius / Math.sin(fov / 2);

      // Add a little padding so the object is not touching the screen edge
      distance *= 1.1;
    }

    viz.camera.position.copy(center).add(lookDir.multiplyScalar(distance));
    viz.orbitControls!.target.copy(center);
    viz.camera.lookAt(center);
    viz.orbitControls!.update();
  };

  const AXES = {
    x: new THREE.Vector3(1, 0, 0),
    y: new THREE.Vector3(0, 1, 0),
    z: new THREE.Vector3(0, 0, 1),
  } as const;

  const snapView = (axis: 'x' | 'y' | 'z') => {
    if (!viz.orbitControls) {
      return;
    }

    const axisVec = AXES[axis];
    const viewDir = new THREE.Vector3().subVectors(viz.orbitControls.target, viz.camera.position).normalize();
    const dot = viewDir.dot(axisVec);

    let sideSign: 1 | -1 = 1;
    if (Math.abs(Math.abs(dot) - 1) < 1e-3) {
      sideSign = dot < 0 ? -1 : 1;
    }

    const distance = viz.camera.position.distanceTo(viz.orbitControls.target);

    viz.camera.position.copy(viz.orbitControls.target).addScaledVector(axisVec, distance * sideSign);
    viz.camera.lookAt(viz.orbitControls.target);
  };

  const orbit = (axis: 'vertical' | 'horizontal', angle: number) => {
    if (!viz.orbitControls) {
      return;
    }

    const camera = viz.camera;
    const target = viz.orbitControls.target;

    const offset = new THREE.Vector3().subVectors(camera.position, target);
    const s = new THREE.Spherical().setFromVector3(offset);

    if (axis === 'horizontal') {
      s.theta += angle;

      const minAz = viz.orbitControls.minAzimuthAngle ?? -Infinity;
      const maxAz = viz.orbitControls.maxAzimuthAngle ?? Infinity;
      s.theta = Math.max(minAz, Math.min(maxAz, s.theta));
    } else {
      s.phi += angle;

      const minPol = viz.orbitControls.minPolarAngle ?? 0;
      const maxPol = viz.orbitControls.maxPolarAngle ?? Math.PI;
      s.phi = Math.max(minPol, Math.min(maxPol, s.phi));
    }

    offset.setFromSpherical(s);
    camera.position.copy(target).add(offset);
    camera.lookAt(target);

    viz.orbitControls.update();
  };

  const toggleWireframe = () => {
    materialOverride = materialOverride === 'wireframe' ? null : 'wireframe';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        const mat =
          materialOverride === 'wireframe'
            ? WireframeMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? HiddenMat);
        obj.material = mat;
      }
    }
  };

  const toggleNormalMat = () => {
    materialOverride = materialOverride === 'normal' ? null : 'normal';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        const mat =
          materialOverride === 'normal'
            ? NormalMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? HiddenMat);
        obj.material = mat;
      }
    }
  };

  let materialEditorOpen = $state(false);
  let materialDefinitions = $state<MaterialDefinitions>(initialMatDefs);

  onMount(() => {
    const referencedTextureIDs: TextureID[] = [];
    for (const mat of Object.values(materialDefinitions.materials)) {
      if (mat.type === 'basic') {
        continue;
      }

      if (mat.map) {
        referencedTextureIDs.push(mat.map);
      }
      if (mat.normalMap) {
        referencedTextureIDs.push(mat.normalMap);
      }
      if (mat.roughnessMap) {
        referencedTextureIDs.push(mat.roughnessMap);
      }
    }

    if (referencedTextureIDs.length > 0) {
      fetchAndSetTextures(referencedTextureIDs);
    }
  });

  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    // TODO: only do this if these have changed
    repl.setMaterials(
      ctxPtr,
      materialDefinitions.defaultMaterialID,
      Object.values(materialDefinitions.materials).map(mat => mat.name)
    );
  });

  interface MatEntry {
    promise: Promise<THREE.Material>;
    resolved: THREE.Material | null;
    beforeRenderCb?: (curTimeSeconds: number) => void;
  }

  const loader = new THREE.ImageBitmapLoader();
  let customMaterials: Record<string, MatEntry> = $derived.by(() => {
    const builtMats: Record<string, MatEntry> = {};

    // TODO: needs hashing to avoid re-building materials that haven't changed
    // `$state.snapshot` seems required here in order to trigger this derived to actually run when things change
    for (const [id, def] of Object.entries($state.snapshot(materialDefinitions.materials))) {
      const matMaybeP = buildMaterial(loader, def, id);
      const entry: MatEntry = {
        promise: matMaybeP instanceof Promise ? matMaybeP : Promise.resolve(matMaybeP),
        resolved: matMaybeP instanceof Promise ? null : matMaybeP,
      };

      const maybeRegisterBeforeRenderCb = (mat: THREE.Material) => {
        if (
          !(mat instanceof CustomShaderMaterial || mat instanceof CustomBasicShaderMaterial) ||
          !def.shaders
        ) {
          return;
        }

        if (
          def.shaders.color ||
          (def.type === 'physical' &&
            (def.shaders.iridescence || def.shaders.metalness || def.shaders.roughness))
        ) {
          const beforeRenderCb = (curTimeSeconds: number) => mat.setCurTimeSeconds(curTimeSeconds);
          viz.registerBeforeRenderCb(beforeRenderCb);
          entry.beforeRenderCb = beforeRenderCb;
        }
      };

      if (matMaybeP instanceof Promise) {
        matMaybeP.then(mat => {
          maybeRegisterBeforeRenderCb(mat);
          entry.resolved = mat;
        });
      } else {
        maybeRegisterBeforeRenderCb(matMaybeP);
      }
      builtMats[id] = entry;
    }
    return builtMats;
  });

  // avoid a ton of before render callbacks from being stuck around, which also prevents
  // old materials from being garbage collected
  $effect(() => {
    const customMatVals = Object.values(customMaterials);
    return () => {
      for (const matEntry of customMatVals) {
        if (matEntry.beforeRenderCb) {
          viz.unregisterBeforeRenderCb(matEntry.beforeRenderCb);
        }
      }
    };
  });

  let didInitMats = false;
  $effect(() => {
    // force dependency
    if ($state.snapshot(materialDefinitions)) {
      if (!didInitMats) {
        didInitMats = true;
      } else {
        isDirty = true;
      }
    } else {
      throw new Error('unreachable');
    }
  });

  let customMaterialsByName: Record<
    string,
    { promise: Promise<THREE.Material>; resolved: THREE.Material | null }
  > = $derived.by(() => {
    const matsByName: Record<string, { promise: Promise<THREE.Material>; resolved: THREE.Material | null }> =
      {};
    for (const [id, def] of Object.entries($state.snapshot(materialDefinitions.materials))) {
      matsByName[def.name] = customMaterials[id];
    }
    return matsByName;
  });

  $effect(() => {
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) {
        continue;
      }

      for (const [id, matEntry] of Object.entries(customMaterials)) {
        if (obj.material.name === id) {
          if (matEntry.resolved) {
            obj.material = matEntry.resolved;
          } else {
            matEntry.promise.then(mat => {
              obj.material = mat;
            });
          }
          break;
        }
      }
    }
  });

  let lastRunOutcome = $derived(
    (() => {
      if (err) {
        return { type: 'err' as const, err };
      }
      if (runStats) {
        return { type: 'ok' as const, stats: runStats };
      }
      return null;
    })()
  );

  const run = async (code?: string) => {
    if (isRunning || ctxPtr === null) {
      return;
    }

    if (typeof code !== 'string') {
      if (editorView) {
        code = editorView.state.doc.toString();
      } else {
        code = lastCode;
      }
    }

    beforeUnloadHandler();

    isRunning = true;
    for (const obj of renderedObjects) {
      viz.scene.remove(obj);
      if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) {
        obj.geometry.dispose();
      }
    }
    renderedObjects = [];

    await repl.reset(ctxPtr);
    runStats = null;
    const startTime = performance.now();
    setLastRunWasSuccessful(false, userData);
    try {
      await repl.eval(ctxPtr, code, includePrelude);
    } catch (err) {
      console.error('Error evaluating code:', err);
      // TODO: this set isn't working for some reason
      err = `Error evaluating code: ${err}`;
      isRunning = false;
      return;
    } finally {
      setLastRunWasSuccessful(true, userData);
    }
    err = (await repl.getErr(ctxPtr)) || null;
    if (err) {
      isRunning = false;
      return;
    }

    const localRunStats: RunStats = {
      runtimeMs: performance.now() - startTime,
      renderedMeshCount: 0,
      renderedPathCount: 0,
      renderedLightCount: 0,
      totalVtxCount: 0,
      totalFaceCount: 0,
    };

    const overrideMat = (() => {
      if (materialOverride === 'wireframe') {
        return WireframeMat;
      }
      if (materialOverride === 'normal') {
        return NormalMat;
      }
      return null;
    })();

    localRunStats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
    const newRenderedMeshes: (
      | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
      | THREE.Line<THREE.BufferGeometry, THREE.Material>
      | THREE.Light
    )[] = [];
    for (let i = 0; i < localRunStats.renderedMeshCount; i += 1) {
      const {
        transform,
        verts,
        indices,
        normals,
        material: materialName,
      } = await repl.getRenderedMesh(ctxPtr, i);

      localRunStats.totalVtxCount += verts.length / 3;
      localRunStats.totalFaceCount += indices.length / 3;

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      if (normals) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      }

      const matEntry = (() => {
        if (!materialName) {
          return { resolved: FallbackMat, promise: null };
        }

        const matEntry = customMaterialsByName[materialName];
        if (!matEntry) {
          console.warn(`mesh referenced undefined material: "${materialName}"`);
          return { resolved: FallbackMat, promise: null };
        }

        return matEntry;
      })();

      const mesh: THREE.Mesh<THREE.BufferGeometry, THREE.Material> = new THREE.Mesh(
        geometry,
        overrideMat ? overrideMat : (matEntry.resolved ?? HiddenMat)
      );
      mesh.userData.materialName = materialName;
      if (!matEntry.resolved && matEntry.promise) {
        matEntry.promise.then(mat => {
          mesh.material = mat;
        });
      }
      mesh.applyMatrix4(new THREE.Matrix4().fromArray(transform));
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      viz.scene.add(mesh);
      newRenderedMeshes.push(mesh);
    }

    localRunStats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
    for (let i = 0; i < localRunStats.renderedPathCount; i += 1) {
      const pathVerts: Float32Array = await repl.getRenderedPathVerts(ctxPtr, i);
      localRunStats.totalVtxCount += pathVerts.length / 3;
      localRunStats.totalFaceCount += pathVerts.length / 3 - 1;
      const pathGeometry = new THREE.BufferGeometry();
      pathGeometry.setAttribute('position', new THREE.BufferAttribute(pathVerts, 3));
      const pathMaterial = LineMat;
      const pathMesh = new THREE.Line(pathGeometry, pathMaterial);
      pathMesh.castShadow = false;
      pathMesh.receiveShadow = false;
      viz.scene.add(pathMesh);
      newRenderedMeshes.push(pathMesh);
    }

    localRunStats.renderedLightCount = await repl.getRenderedLightCount(ctxPtr);
    for (let i = 0; i < localRunStats.renderedLightCount; i += 1) {
      const light = await repl.getRenderedLight(ctxPtr, i);
      const builtLight = buildAndAddLight(viz, light, userData?.renderMode ?? false);
      newRenderedMeshes.push(builtLight);
    }

    renderedObjects = newRenderedMeshes;
    runStats = localRunStats;
    isRunning = false;
  };

  const toggleEditorCollapsed = () => {
    if (editorView) {
      lastCode = editorView.state.doc.toString();
      saveState(
        {
          code: lastCode,
          materials: materialDefinitions,
          view: getView(viz),
        },
        userData
      );
    }
    isEditorCollapsed = !isEditorCollapsed;
    onHeightChange(height, isEditorCollapsed);
  };

  const toggleAxisHelpers = () => {
    const helper = viz.scene.children.find(obj => obj instanceof THREE.AxesHelper);
    if (helper) {
      viz.scene.remove(helper);
      localStorage['geoscript-axis-helpers'] = 'false';
    } else {
      const axisHelper = new THREE.AxesHelper(100);
      axisHelper.position.set(0, 0, 0);
      viz.scene.add(axisHelper);
      localStorage['geoscript-axis-helpers'] = 'true';
    }
  };

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const setView = async (view: CompositionVersionMetadata['view']) => {
    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    viz.camera.position.set(...view.cameraPosition);
    viz.orbitControls.target.set(...view.target);
    if ('fov' in viz.camera && view.fov !== undefined) {
      viz.camera.fov = view.fov;
      viz.camera.updateProjectionMatrix();
    }
    if ('zoom' in viz.camera && view.zoom !== undefined) {
      viz.camera.zoom = view.zoom;
      viz.camera.updateProjectionMatrix();
    }
    viz.camera.lookAt(viz.orbitControls.target);
    viz.orbitControls.update();
  };

  const clearLocalChanges = () => {
    if (isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    clearSavedState(userData);

    const serverState = getServerState(userData);

    if (editorView) {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: serverState.code },
      });
    }
    didInitMats = false;

    materialDefinitions = serverState.materials;
    const missingTextureIDs = Object.values(materialDefinitions.materials).flatMap(mat => {
      if (mat.type === 'basic') {
        return [];
      }
      const referencedTextureIDs: TextureID[] = [];
      if (mat.map) {
        referencedTextureIDs.push(mat.map);
      }
      if (mat.normalMap) {
        referencedTextureIDs.push(mat.normalMap);
      }
      if (mat.roughnessMap) {
        referencedTextureIDs.push(mat.roughnessMap);
      }
      return referencedTextureIDs;
    });
    fetchAndSetTextures(missingTextureIDs).then(() => {
      didInitMats = false;
      materialDefinitions = { ...serverState.materials };
    });

    setView(serverState.view);

    run(serverState.code);

    saveState(
      {
        code: serverState.code,
        materials: serverState.materials,
        view: serverState.view,
      },
      userData
    );

    isDirty = false;
  };

  onMount(() => {
    if (userData?.renderMode) {
      const stats = document.getElementById('viz-stats');
      if (stats) {
        stats.style.display = 'none';
      }
    }

    setTimeout(() => setView(initialView));

    setReplCtx({
      centerView,
      toggleWireframe,
      toggleNormalMat,
      getLastRunOutcome: () => lastRunOutcome,
      getAreAllMaterialsLoaded: () => Object.values(customMaterials).every(mat => mat.resolved),
      run,
      snapView,
      orbit,
    });

    window.addEventListener('beforeunload', beforeUnloadHandler);

    return () => {
      for (const mesh of renderedObjects) {
        viz.scene.remove(mesh);
        if (mesh instanceof THREE.Mesh || mesh instanceof THREE.Line) {
          mesh.geometry.dispose();
        }
      }

      window.removeEventListener('beforeunload', beforeUnloadHandler);
    };
  });

  const goHome = () => {
    beforeUnloadHandler();

    if (isDirty) {
      if (!confirm('You have unsaved changes. Really leave page?')) {
        return;
      }
    }

    goto('/geotoy');
  };
</script>

<svelte:window bind:innerWidth />

<ExportModal bind:dialog={exportDialog} {renderedObjects} />
<MaterialEditor bind:isOpen={materialEditorOpen} bind:materials={materialDefinitions} />

{#if isEditorCollapsed}
  <div
    class="root collapsed"
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} height: 36px;`}
  >
    <ReplControls
      {isRunning}
      {isEditorCollapsed}
      {run}
      {toggleEditorCollapsed}
      {goHome}
      {err}
      {onExport}
      {clearLocalChanges}
      {toggleAxisHelpers}
      {isDirty}
      toggleMaterialEditorOpen={() => (materialEditorOpen = true)}
    />
  </div>
{:else}
  <div
    class="root"
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} height: ${height}px;`}
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="dragger" role="separator" aria-orientation="horizontal" onmousedown={handleMousedown}></div>
    <div class="editor-container">
      <div
        bind:this={codemirrorContainer}
        class="codemirror-wrapper"
        style="flex: 1; background: #222;"
      ></div>
      <div class="controls">
        <div class="output">
          <ReplControls
            {isRunning}
            {isEditorCollapsed}
            {run}
            {toggleEditorCollapsed}
            {goHome}
            {err}
            {onExport}
            {clearLocalChanges}
            {toggleAxisHelpers}
            {isDirty}
            toggleMaterialEditorOpen={() => {
              materialEditorOpen = !materialEditorOpen;
            }}
          />
          <ReplOutput {err} {runStats} />
        </div>
        {#if userData?.me && (!userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id)}
          <SaveControls
            comp={userData.initialComposition?.comp}
            getCurrentCode={() => editorView?.state.doc.toString() || ''}
            materials={materialDefinitions}
            {viz}
            onSave={() => {
              isDirty = false;
              saveState(
                {
                  code: editorView?.state.doc.toString() || '',
                  materials: materialDefinitions,
                  view: getView(viz),
                },
                userData
              );
            }}
          />
        {:else if !userData?.me}
          <div class="not-logged-in" style="border-top: 1px solid #333">
            <span style="color: #ddd">you must be logged in to save/share compositions</span>
            <div>
              <a href="/geotoy/login">log in</a>
              /
              <a href="/geotoy/register">register</a>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style lang="css">
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  .root {
    width: 100%;
    position: absolute;
    max-width: 100vw;
    overflow-x: hidden;
    bottom: 0;
    display: flex;
    flex-direction: column;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 15px;
  }

  .dragger {
    width: 100%;
    height: 5px;
    position: absolute;
    top: -2px;
    left: 0;
    cursor: ns-resize;
    z-index: 2;
  }

  .editor-container {
    display: flex;
    flex-direction: row;
    flex: 1;
    min-height: 0;
  }

  .output {
    display: flex;
    flex-direction: column;
    flex: 1;
    padding: 8px;
    overflow-y: auto;
    min-height: 80px;
  }

  .codemirror-wrapper {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    overflow-x: auto;
    background: #222;
  }

  :global(.codemirror-wrapper > div) {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
  }

  :global(.cm-content) {
    padding-top: 0 !important;
  }

  .controls {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    flex: 0.4;
    border-top: 1px solid #444;
    overflow-y: auto;
  }

  .not-logged-in {
    font-size: 13px;
    padding: 8px;
  }

  @media (max-width: 768px) {
    .editor-container {
      flex-direction: column;
    }

    .output {
      padding: 4px;
    }

    .codemirror-wrapper {
      flex: 1;
    }

    .controls {
      flex: 1;
      border-top: none;
      border-left: 1px solid #444;
    }

    .not-logged-in {
      font-size: 12px;
      padding: 4px;
    }

    .output {
      overflow-x: hidden;
    }
  }
</style>
