<script lang="ts">
  import { resolve } from '$app/paths';

  import { deleteComposition } from 'src/geoscript/geotoyAPIClient';
  import { showToast } from 'src/viz/util/GlobalToastState.svelte';
  import GeotoyHeader from '../../GeotoyHeader.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  let compositions = $derived(data.compositions);

  const doDeleteComposition = async (compositionID: number) => {
    if (!confirm('Are you sure you want to delete this composition?')) {
      return;
    }

    try {
      await deleteComposition(compositionID);

      compositions = compositions.filter(c => c.comp.id !== compositionID);
      showToast({ status: 'success', message: 'Composition deleted' });
    } catch (err) {
      console.error('Failed to delete composition:', err);
      showToast({ status: 'error', message: 'Failed to delete composition' });
    }
  };
</script>

<svelte:head>
  <title>{data.user.username}'s Compositions</title>
</svelte:head>

<div class="root">
  <GeotoyHeader me={data.me} showTitleLink />

  <div class="content">
    <h2>{data.user.username}</h2>

    <ul class="composition-list">
      {#each compositions as { comp, latest } (comp.id)}
        <li class="composition-item">
          <a href={resolve(`/geotoy/edit/${comp.id}`)} tabindex="-1" class="thumbnail-link">
            {#if latest.thumbnail_url}
              <img
                src={latest.thumbnail_url}
                alt={comp.description || 'Composition thumbnail'}
                class="thumbnail"
                crossorigin="anonymous"
                loading="lazy"
                width="70"
                height="70"
              />
            {:else}
              <div class="thumbnail placeholder"></div>
            {/if}
          </a>

          <div class="info">
            <a href={resolve(`/geotoy/edit/${comp.id}`)} class="title-text">{comp.title}</a>
            <div class="meta">
              <span class="date">last updated {new Date(comp.updated_at).toLocaleDateString()}</span>
              <span class="status" style={`color: ${comp.is_shared ? '#12cc12' : 'red'}`}>
                {comp.is_shared ? 'public' : 'private'}
              </span>
            </div>
          </div>

          <div class="actions">
            <a href={resolve(`/geotoy/history/${comp.id}`)} class="action-link">history</a>
            {#if data.isMe}
              <button class="delete-btn" onclick={() => doDeleteComposition(comp.id)}>delete</button>
            {/if}
          </div>
        </li>
      {/each}
      {#if compositions.length === 0}
        <li class="empty-message">{data.isMe ? 'you have no compositions' : 'no public compositions'}</li>
      {/if}
    </ul>
  </div>
</div>

<style>
  .root {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    padding: 0 8px 8px 8px;
    box-sizing: border-box;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  .content {
    max-width: 900px;
    width: 100%;
    margin: 0 auto;
    padding: 16px 8px;
  }

  h2 {
    text-align: left;
    margin: 0 0 12px 0;
    font-size: 24px;
    font-weight: 400;
    border-bottom: 1px solid #555;
    padding-bottom: 4px;
  }

  .composition-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .composition-item {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 6px 0;
    border-bottom: 1px solid #2a2a2a;
  }

  .composition-item:last-child {
    border-bottom: none;
  }

  .thumbnail-link {
    display: block;
    flex-shrink: 0;
  }

  .thumbnail {
    width: 70px;
    height: 70px;
    object-fit: cover;
    display: block;
    background: #222;
  }

  .placeholder {
    background: #222
      repeating-linear-gradient(-45deg, transparent, transparent 9px, #181818 9px, #181818 18px);
  }

  .info {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .title-text {
    font-size: 16px;
    font-weight: 600;
    color: #f0f0f0;
    text-decoration: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  a.title-text:hover {
    color: #fff;
    text-decoration: underline;
  }

  .meta {
    font-size: 12px;
    color: #ccc;
    display: flex;
    flex-wrap: wrap;
    gap: 32px;
  }

  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .action-link {
    color: #7cb3f0;
    text-decoration: none;
    font-size: 12px;
  }

  .action-link:hover {
    text-decoration: underline;
  }

  .delete-btn {
    background: none;
    border: 1px solid #555;
    color: #ccc;
    font-size: 12px;
    padding: 4px 8px;
    cursor: pointer;
    font-family: inherit;
  }

  .delete-btn:hover {
    background: #9b111e;
    border-color: #c51829;
    color: #fff;
  }

  .empty-message {
    color: #888;
    padding: 40px 0;
    text-align: center;
    font-style: italic;
    border-bottom: 1px solid #2a2a2a;
  }

  @media (max-width: 600px) {
    .content {
      padding: 12px 4px;
    }

    h2 {
      font-size: 20px;
    }

    .composition-item {
      flex-wrap: wrap;
      row-gap: 12px;
    }

    .thumbnail-link {
      flex-shrink: 0;
    }

    .thumbnail,
    .placeholder {
      width: 50px;
      height: 50px;
    }

    .info {
      flex-grow: 1;
    }

    .title-text {
      font-size: 15px;
    }

    .meta {
      font-size: 11px;
      gap: 16px;
    }

    .actions {
      flex-basis: 100%;
      margin-left: 0;
      display: flex;
      justify-content: flex-end;
    }
  }
</style>
