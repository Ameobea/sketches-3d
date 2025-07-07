<script lang="ts">
  import {
    APIError,
    createCompositionVersion,
    type Composition,
    type CompositionVersion,
    type CompositionVersionMetadata,
    type User,
  } from 'src/geoscript/geotoyAPIClient';
  import type { Viz } from 'src/viz';
  import { OrthographicCamera, PerspectiveCamera } from 'three';
  import type { OrbitControls } from 'three/examples/jsm/Addons.js';

  let {
    viz,
    comp,
    version,
    me,
    getCurrentCode,
    onSave,
  }: {
    viz: Viz;
    comp: Composition;
    version: CompositionVersion;
    me: User;
    getCurrentCode: () => string;
    onSave: (savedSrc: string) => void;
  } = $props();

  let status = $state<
    { type: 'ok'; msg: string; seq: number } | { type: 'error'; msg: string } | { type: 'loading' } | null
  >(null);

  const handleSave = async () => {
    status = { type: 'loading' };
    const seq = Math.random();

    try {
      const code = getCurrentCode();
      const controls: OrbitControls | null = viz.orbitControls;
      if (!controls) {
        status = { type: 'error', msg: 'missing orbit controls; app not yet initialized?' };
        return;
      }
      const view: CompositionVersionMetadata['view'] = {
        cameraPosition: [viz.camera.position.x, viz.camera.position.y, viz.camera.position.z],
        target: [controls.target.x, controls.target.y, controls.target.z],
      };
      if (viz.camera instanceof PerspectiveCamera) {
        view.fov = (viz.camera as any).fov;
      }
      if (viz.camera instanceof OrthographicCamera) {
        view.zoom = (viz.camera as any).zoom;
      }
      const metadata: CompositionVersionMetadata = {
        view,
      };

      await createCompositionVersion(comp.id, { source_code: code, metadata });
      onSave(code);

      status = { type: 'ok', msg: 'Changes saved successfully!', seq };
      setTimeout(() => {
        if (status?.type === 'ok' && status?.seq === seq) {
          status = null;
        }
      }, 2200);
    } catch (error) {
      console.error('Error saving changes:', error);
      if (error instanceof APIError) {
        status = { type: 'error', msg: error.message };
      } else {
        status = { type: 'error', msg: `${error}` };
      }
    }
  };
</script>

<div class="root">
  <div class="buttons">
    <button onclick={handleSave} disabled={status?.type === 'loading'}>save</button>
  </div>
  {#if status && (status.type === 'ok' || status.type === 'error')}
    <div class="status {status.type}">
      {status.msg}
    </div>
  {:else}
    <div style="height: 8px"></div>
  {/if}
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    flex: 0;
    border-top: 1px solid #333;
    padding: 8px;
  }

  .status {
    font-size: 12px;
    padding: 2px;
    margin-top: 4px;
  }

  .status.ok {
    color: #12cc12;
  }

  .status.error {
    color: red;
  }
</style>
