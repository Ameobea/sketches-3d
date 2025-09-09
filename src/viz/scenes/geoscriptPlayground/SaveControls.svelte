<script lang="ts">
  import { goto } from '$app/navigation';
  import {
    APIError,
    createComposition,
    createCompositionVersion,
    updateComposition,
    type Composition,
    type CompositionVersionMetadata,
  } from 'src/geoscript/geotoyAPIClient';
  import type { MaterialDefinitions } from 'src/geoscript/materials';
  import type { Viz } from 'src/viz';
  import { OrthographicCamera, PerspectiveCamera } from 'three';
  import type { OrbitControls } from 'three/examples/jsm/Addons.js';

  let {
    viz,
    comp,
    materials,
    getCurrentCode,
    onSave,
    preludeEjected,
  }: {
    viz: Viz;
    comp: Composition | undefined | null;
    materials: MaterialDefinitions;
    getCurrentCode: () => string;
    onSave?: (savedSrc: string) => void;
    preludeEjected: boolean;
  } = $props();

  let title = $state(comp?.title || '');
  let description = $state(comp?.description || '');
  let isShared = $state(comp?.is_shared || false);

  let status = $state<
    { type: 'ok'; msg: string; seq: number } | { type: 'error'; msg: string } | { type: 'loading' } | null
  >(null);

  const buildCompositionVersionMetadata = (
    viz: Viz
  ): { type: 'ok'; metadata: CompositionVersionMetadata } | { type: 'error'; msg: string } => {
    const controls: OrbitControls | null = viz.orbitControls;
    if (!controls) {
      return { type: 'error', msg: 'missing orbit controls; app not yet initialized?' };
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
      materials,
      preludeEjected,
    };

    return { type: 'ok', metadata };
  };

  const saveNewVersion = async (
    comp: Composition
  ): Promise<{ type: 'ok' } | { type: 'error'; msg: string }> => {
    try {
      const code = getCurrentCode();

      const metadataRes = buildCompositionVersionMetadata(viz);
      if (metadataRes.type === 'error') {
        return metadataRes;
      }
      const metadata = metadataRes.metadata;

      await Promise.all([
        createCompositionVersion(comp.id, { source_code: code, metadata }),
        updateComposition(comp.id, ['title', 'description', 'is_shared'], {
          title,
          description,
          is_shared: isShared,
        }),
      ]);
      onSave?.(code);
      return { type: 'ok' };
    } catch (error) {
      console.error('Error saving changes:', error);
      if (error instanceof APIError) {
        return { type: 'error', msg: error.message };
      } else {
        return { type: 'error', msg: `${error}` };
      }
    }
  };

  const createNewComposition = async (): Promise<
    { type: 'ok'; comp: Composition } | { type: 'error'; msg: string }
  > => {
    const metadataRes = buildCompositionVersionMetadata(viz);
    if (metadataRes.type === 'error') {
      return metadataRes;
    }
    try {
      const comp = await createComposition({
        title,
        description,
        metadata: metadataRes.metadata,
        source_code: getCurrentCode(),
        is_shared: isShared,
      });
      return { type: 'ok', comp };
    } catch (error) {
      console.error('Error creating new composition:', error);
      if (error instanceof APIError) {
        return { type: 'error', msg: error.message };
      } else {
        return { type: 'error', msg: `${error}` };
      }
    }
  };

  const handleSave = async () => {
    status = { type: 'loading' };
    const seq = Math.random();

    const res = await (async (): Promise<{ type: 'ok'; msg: string } | { type: 'error'; msg: string }> => {
      if (comp) {
        const res = await saveNewVersion(comp);
        if (res.type === 'error') {
          return res;
        }
        return { type: 'ok', msg: 'Successfully created new version' };
      } else {
        const compRes = await createNewComposition();
        if (compRes.type === 'error') {
          return { type: 'error', msg: compRes.msg };
        }
        const comp = compRes.comp;
        goto(`/geotoy/edit/${comp.id}`);
        return { type: 'ok', msg: 'Successfully saved new composition' };
      }
    })();

    if (res.type === 'error') {
      status = { type: 'error', msg: res.msg };
    } else {
      status = { type: 'ok', msg: res.msg, seq };
      setTimeout(() => {
        if (status?.type === 'ok' && status?.seq === seq) {
          status = null;
        }
      }, 2200);
    }
  };
</script>

<div class="root">
  <div class="form-grid">
    <label for="is-shared-checkbox">public</label>
    <input id="is-shared-checkbox" type="checkbox" bind:checked={isShared} />

    <label for="title-input">title</label>
    <input id="title-input" type="text" bind:value={title} />

    <label for="description-input">description</label>
    <textarea id="description-input" rows="3" bind:value={description}></textarea>
  </div>

  <div class="controls">
    <div class="status-container">
      {#if status && (status.type === 'ok' || status.type === 'error')}
        <div class="status {status.type}">
          {status.msg}
        </div>
      {/if}
    </div>
    <button onclick={handleSave} disabled={status?.type === 'loading'}>
      {#if status?.type === 'loading'}
        saving...
      {:else}
        save
      {/if}
    </button>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    flex: 0;
    border-top: 1px solid #333;
    padding: 8px;
    gap: 6px;
  }

  .form-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px;
    align-items: center;
  }

  label {
    font-size: 12px;
    text-align: right;
    white-space: nowrap;
  }

  input[type='text'],
  textarea {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 3px 4px;
    font-size: 12px;
    font-family: inherit;
    width: 100%;
    box-sizing: border-box;
  }

  textarea {
    resize: vertical;
    min-height: 40px;
  }

  input[type='text']:focus,
  textarea:focus {
    outline: none;
    border-color: #777;
    background-color: #222;
  }

  .controls {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 16px;
    min-height: 28px;
  }

  button {
    background-color: #2a2a2a;
    color: #eee;
    border: 1px solid #555;
    padding: 4px 8px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background-color: #333;
    border-color: #777;
  }

  button:disabled {
    background-color: #222;
    color: #666;
    border-color: #444;
    cursor: not-allowed;
  }

  .status-container {
    flex-grow: 1;
  }

  .status {
    font-size: 12px;
  }

  .status.ok {
    color: #12cc12;
  }

  .status.error {
    color: red;
  }

  @media (max-width: 600px) {
    .root {
      padding: 4px;
      gap: 4px;
    }

    .form-grid {
      grid-template-columns: auto 1fr;
      gap: 4px;
    }

    label {
      font-size: 11px;
    }

    input[type='text'],
    textarea {
      font-size: 11px;
    }

    button {
      font-size: 11px;
      padding: 3px 6px;
    }
  }
</style>
