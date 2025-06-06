<script lang="ts" module>
  export interface ReplCtx {
    centerView: () => void;
    toggleWireframe: () => void;
  }

  const DefaultCameraPos = new THREE.Vector3(10, 10, 10);
  const DefaultCameraTarget = new THREE.Vector3(0, 0, 0);
</script>

<script lang="ts">
  import type { Viz } from 'src/viz';
  import { buildGrayFossilRockMaterial } from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
  import * as THREE from 'three';
  import { gruvboxDark } from 'cm6-theme-gruvbox-dark';
  import { json } from '@codemirror/lang-json';
  import { onMount } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView, keymap } from '@codemirror/view';
  import { defaultKeymap } from '@codemirror/commands';
  import { basicSetup } from 'codemirror';
  import {
    foldNodeProp,
    foldInside,
    indentNodeProp,
    LRLanguage,
    LanguageSupport,
  } from '@codemirror/language';
  import { styleTags, tags as t } from '@lezer/highlight';

  import { parser } from './parser/geoscript';

  let {
    viz,
    repl,
    ctxPtr,
    setReplCtx,
  }: {
    viz: Viz;
    repl: typeof import('src/viz/wasmComp/geoscript_repl');
    ctxPtr: number;
    setReplCtx: (ctx: ReplCtx) => void;
  } = $props();

  let err: string | null = $state(null);
  let renderedMeshes: THREE.Mesh<THREE.BufferGeometry, THREE.Material>[] = $state([]);

  const loader = new THREE.ImageBitmapLoader();
  const matPromise = buildGrayFossilRockMaterial(
    loader,
    { uvTransform: new THREE.Matrix3().scale(0.2, 0.2), color: 0xcccccc, mapDisableDistance: null },
    {},
    { useGeneratedUVs: true, useTriplanarMapping: false, tileBreaking: undefined }
  );

  let codemirrorContainer: HTMLDivElement | null = $state(null);
  let editorState = $state<EditorState | null>(null);
  let editorView: EditorView | null = $state(null);
  let currentMaterial: THREE.Material | null = $state(null);

  const computeCompositeBoundingBox = (
    meshes: THREE.Mesh<THREE.BufferGeometry, THREE.Material>[]
  ): THREE.Box3 => {
    const box = new THREE.Box3();
    for (const mesh of meshes) {
      mesh.geometry.computeBoundingBox();
      const meshBox = mesh.geometry.boundingBox;
      if (meshBox) box.union(meshBox.applyMatrix4(mesh.matrixWorld));
    }
    return box;
  };

  const centerView = async () => {
    if (!viz) {
      return;
    }

    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    if (!renderedMeshes.length) {
      viz.camera.position.copy(DefaultCameraPos);
      viz.orbitControls!.target.copy(DefaultCameraTarget);
      viz.camera.lookAt(DefaultCameraTarget);
      viz.orbitControls!.update();
      return;
    }

    const compositeBbox = computeCompositeBoundingBox(renderedMeshes);
    const center = new THREE.Vector3();
    compositeBbox.getCenter(center);
    const size = new THREE.Vector3();
    compositeBbox.getSize(size);

    // try to keep the same look direction
    const lookDir = new THREE.Vector3();
    lookDir.copy(viz.orbitControls!.target).sub(viz.camera.position).normalize();
    const maxDim = Math.max(size.x, size.y, size.z);
    const distance = maxDim * 1.2 + 1;
    viz.camera.position.copy(center).sub(lookDir.multiplyScalar(distance));
    viz.orbitControls!.target.copy(center);
    viz.camera.lookAt(center);
    viz.orbitControls!.update();
  };

  const toggleWireframe = () => {
    for (const mesh of renderedMeshes) {
      if (mesh.material && 'wireframe' in mesh.material) {
        mesh.material.wireframe = !mesh.material.wireframe;
        mesh.material.needsUpdate = true;
      }
    }
  };

  const run = async () => {
    if (!editorView || !editorState) {
      return;
    }

    const code = editorView.state.doc.toString();
    const tree = parser.parse(code);
    let cursor = tree.cursor();
    do {
      console.log(`Node ${cursor.name} from ${cursor.from} to ${cursor.to}`);
    } while (cursor.next());

    for (const mesh of renderedMeshes) {
      viz.scene.remove(mesh);
      mesh.geometry.dispose();
    }
    renderedMeshes = [];

    repl.geoscript_repl_reset(ctxPtr);
    try {
      repl.geoscript_repl_eval(ctxPtr, code);
    } catch (err) {
      console.error('Error evaluating code:', err);
      // TODO: this set isn't working for some reason
      err = `Error evaluating code: ${err}`;
      return;
    }
    err = repl.geoscript_repl_get_err(ctxPtr) || null;

    const renderedMeshCount = repl.geoscript_repl_get_rendered_mesh_count(ctxPtr);
    const newRenderedMeshes = [];
    for (let i = 0; i < renderedMeshCount; i++) {
      const verts = repl.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, i);
      const indices = repl.geoscript_repl_get_rendered_mesh_indices(ctxPtr, i);
      const normals = repl.geoscript_repl_get_rendered_mesh_normals(ctxPtr, i);

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      if (normals) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      }

      const mat = await matPromise;
      const mesh = new THREE.Mesh(geometry, mat);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      viz.scene.add(mesh);
      newRenderedMeshes.push(mesh);
    }

    renderedMeshes = newRenderedMeshes;
  };

  onMount(() => {
    const parserWithMetadata = parser.configure({
      props: [
        styleTags({
          identifier: t.variableName,
          'CallExpr/identifier': t.function(t.variableName),
          LineComment: t.lineComment,
          Integer: t.integer,
          Float: t.float,
        }),
        indentNodeProp.add({
          Application: context => context.column(context.node.from) + context.unit,
        }),
        foldNodeProp.add({
          Application: foldInside,
        }),
      ],
    });

    const geoscriptLang = LRLanguage.define({
      parser: parserWithMetadata,
      languageData: {
        commentTokens: { line: '//' },
      },
    });

    editorState = EditorState.create({
      doc: localStorage.lastGeoscriptPlaygroundCode || '(box(8) + (box(8) | trans(4, 4, -4))) | render',
      extensions: [
        basicSetup,
        keymap.of(defaultKeymap),
        gruvboxDark,
        new LanguageSupport(geoscriptLang),
        // json(),
      ],
    });

    editorView = new EditorView({
      state: editorState,
      parent: codemirrorContainer!,
    });

    setReplCtx({ centerView, toggleWireframe });

    run().then(centerView);

    const beforeUnloadHandler = () => {
      if (editorView) {
        localStorage.lastGeoscriptPlaygroundCode = editorView.state.doc.toString();
      }
    };
    window.addEventListener('beforeunload', beforeUnloadHandler);

    return () => {
      if (editorView) {
        localStorage.lastGeoscriptPlaygroundCode = editorView.state.doc.toString();
        editorView.destroy();
      }
      for (const mesh of renderedMeshes) {
        viz.scene.remove(mesh);
        mesh.geometry.dispose();
      }

      window.removeEventListener('beforeunload', beforeUnloadHandler);
    };
  });
</script>

<div class="root">
  <div
    bind:this={codemirrorContainer}
    class="codemirror-wrapper"
    style="display: flex; flex: 1; background: #222;"
  ></div>
  <div class="controls">
    <button onclick={run}>run</button>
    {#if err}
      <div class="error">{err}</div>
    {/if}
  </div>
</div>

<style lang="css">
  .root {
    height: calc(max(250px, 10vh));
    width: 100%;
    position: absolute;
    bottom: 0;
    display: flex;
    flex-direction: row;
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

  .controls {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    flex: 0.4;
    padding: 8px;
    border-top: 1px solid #444;
  }

  .error {
    color: red;
    background: #222;
    padding: 16px 8px;
    margin-top: 8px;
    overflow-y: auto;
    overflow-x: hidden;
    max-height: 200px;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  button {
    background: #333;
    color: #f0f0f0;
    border: 1px solid #888;
    border-radius: 0;
    padding: 8px 12px;
    cursor: pointer;
    font-size: 14px;
  }
</style>
