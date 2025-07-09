<script lang="ts">
  import type * as THREE from 'three';

  import { exportGLTF, exportOBJ } from './export';

  let {
    dialog = $bindable(),
    renderedObjects,
  }: {
    dialog: HTMLDialogElement | null;
    renderedObjects: (THREE.Mesh | THREE.Line | THREE.Light)[];
  } = $props();

  let exportType = $state<{ type: 'gltf'; binary: boolean } | { type: 'obj' }>({
    type: 'gltf',
    binary: false,
  });

  const exportScene = () => {
    if (exportType.type === 'gltf') {
      exportGLTF(renderedObjects, exportType.binary);
    } else if (exportType.type === 'obj') {
      exportOBJ(renderedObjects);
    } else {
      exportType satisfies never;
    }
    dialog?.close();
  };
</script>

<dialog bind:this={dialog}>
  <div class="modal-content">
    <h2>export options</h2>
    <div class="form-group">
      <label for="export-type">format:</label>
      <select
        id="export-type"
        onchange={e => {
          const value = (e.target as HTMLSelectElement).value;
          if (value === 'gltf') {
            exportType = { type: 'gltf', binary: false };
          } else if (value === 'obj') {
            exportType = { type: 'obj' };
          }
        }}
        value={exportType.type}
      >
        <option value="gltf">gltf</option>
        <option value="obj">obj</option>
      </select>
      {#if exportType.type === 'gltf'}
        <label for="gltf-binary">
          <input
            type="checkbox"
            id="gltf-binary"
            checked={exportType.binary}
            onchange={e => {
              const checked = (e.target as HTMLInputElement).checked;
              exportType = { type: 'gltf', binary: checked };
            }}
          />
          binary (.glb)
        </label>
      {/if}
    </div>
    <div class="buttons">
      <button onclick={() => dialog?.close()}>Cancel</button>
      <button onclick={exportScene}>Export</button>
    </div>
  </div>
</dialog>

<style>
  dialog {
    background: #222;
    color: #f0f0f0;
    border: 1px solid #888;
    padding: 24px;
    width: 80%;
    max-width: 400px;
  }

  dialog::backdrop {
    background: rgba(0, 0, 0, 0.6);
  }

  .modal-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  h2 {
    margin: 0;
    text-align: left;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  select {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 13px;
    border: 1px solid #ccc;
    background: #333;
    color: #f0f0f0;
    padding: 4px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
