<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Snippet } from 'svelte';
  import type { Composition, CompositionVersion } from 'src/geoscript/geotoyAPIClient';
  import { dismissOnEscape } from './dismissOn';
  import ForkCompositionButton from './ForkCompositionButton.svelte';
  import ReadOnlyCompositionDetails from './ReadOnlyCompositionDetails.svelte';

  let {
    comp,
    isOwner,
    loggedIn,
    onClose,
    onForked,
    form,
  }: {
    comp: Composition | null;
    isOwner: boolean;
    loggedIn: boolean;
    onClose: () => void;
    onForked?: (comp: Composition, version: CompositionVersion) => Promise<void>;
    /** The version form; only rendered for the composition's owner. */
    form: Snippet;
  } = $props();

  // Escape only: the form holds unsaved title/description/tags, and unmounting on an outside
  // click discards them.
  $effect(() => dismissOnEscape(onClose));
</script>

<div class="popover">
  <div class="header">{isOwner ? 'save version' : 'composition'}</div>

  {#if isOwner}
    {@render form()}
  {:else if comp}
    <ReadOnlyCompositionDetails {comp} />
  {/if}

  {#if !loggedIn}
    <div class="notice">
      you must be logged in to save/share compositions
      <div>
        <a href={resolve('/geotoy/login')}>log in</a>
        /
        <a href={resolve('/geotoy/register')}>register</a>
      </div>
    </div>
  {/if}

  {#if comp}
    <div class="links">
      {#if onForked && loggedIn}
        <ForkCompositionButton {comp} {onForked} variant="link" />
      {/if}
      <a class="link" href={resolve(`/geotoy/history/${comp.id}`)} target="_blank" rel="noreferrer">
        history
      </a>
    </div>
  {/if}
</div>

<style>
  .popover {
    width: 360px;
    max-width: 100vw;
    background: #222;
    border: 1px solid #555;
    display: flex;
    flex-direction: column;
    max-height: 70vh;
    overflow-y: auto;
    box-sizing: border-box;
  }

  .header {
    font-size: 11px;
    color: #888;
    padding: 6px 8px;
    border-bottom: 1px solid #333;
  }

  .notice {
    font-size: 12px;
    color: #ddd;
    padding: 8px;
  }

  .links {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 8px;
    border-top: 1px solid #333;
  }

  .link {
    color: #888;
    font-size: 11px;
  }

  .link:hover {
    color: #ddd;
  }
</style>
