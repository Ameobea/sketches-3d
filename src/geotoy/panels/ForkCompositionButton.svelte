<script lang="ts">
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { forkComposition, type Composition, type CompositionVersion } from 'src/geoscript/geotoyAPIClient';
  import { showToast } from 'src/viz/util/GlobalToastState.svelte';
  import { logGeotoyEvent } from 'src/analytics';

  let {
    comp,
    onForked,
    variant = 'button',
  }: {
    comp: Composition;
    onForked: (comp: Composition, version: CompositionVersion) => Promise<void>;
    /** `link` is the 11px demoted form used inside the save popover. */
    variant?: 'button' | 'link';
  } = $props();

  let isForking = $state(false);

  const fork = () => {
    if (isForking) {
      return;
    }
    isForking = true;
    forkComposition(comp.id)
      .then(({ composition: newComp, version: newVersion }) => {
        logGeotoyEvent('composition', 'fork', { comp_id: comp.id });
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

<button class={variant} onclick={fork} disabled={isForking}>
  {#if isForking}
    forking...
  {:else if variant === 'link'}
    fork
  {:else}
    fork composition
  {/if}
</button>

<style lang="css">
  button.link {
    background: none;
    border: none;
    color: #888;
    font-size: 11px;
    font-family: inherit;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }

  button.link:hover:not(:disabled) {
    color: #ddd;
  }

  button.button {
    background-color: #2a2a2a;
    color: #eee;
    border: 1px solid #555;
    padding: 4px 8px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  button.button:hover:not(:disabled) {
    background-color: #333;
    border-color: #777;
  }

  button:disabled {
    color: #666;
    cursor: not-allowed;
  }

  button.button:disabled {
    background-color: #222;
    border-color: #444;
  }

  @media (max-width: 600px) {
    button {
      font-size: 11px;
      padding: 3px 6px;
    }
  }
</style>
