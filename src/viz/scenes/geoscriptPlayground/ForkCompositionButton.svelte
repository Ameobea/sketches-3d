<script lang="ts">
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { forkComposition, type Composition, type CompositionVersion } from 'src/geoscript/geotoyAPIClient';
  import { showToast } from 'src/viz/util/GlobalToastState.svelte';

  let {
    comp,
    onForked,
  }: {
    comp: Composition;
    onForked: (comp: Composition, version: CompositionVersion) => Promise<void>;
  } = $props();

  let isForking = $state(false);

  const fork = () => {
    if (isForking) {
      return;
    }
    isForking = true;
    forkComposition(comp.id)
      .then(({ composition: newComp, version: newVersion }) => {
        showToast({ status: 'success', message: 'Successfully forked composition' });
        goto(resolve(`/geotoy/edit/${newComp.id}`), {
          noScroll: true,
          invalidateAll: true,
          keepFocus: false,
        }).then(() => onForked(newComp, newVersion));
      })
      .catch(err => {
        console.error('Error forking composition:', err);
        alert('Error forking composition');
      })
      .finally(() => {
        isForking = false;
      });
  };
</script>

<button onclick={fork} disabled={isForking}>
  {#if isForking}
    forking...
  {:else}
    fork composition
  {/if}
</button>

<style lang="css">
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

  @media (max-width: 600px) {
    button {
      font-size: 11px;
      padding: 3px 6px;
    }
  }
</style>
