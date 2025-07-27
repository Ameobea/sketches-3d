<script lang="ts">
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  let compositions = $state(data.compositions);
  let errorMessage = $state<string | null>(null);

  const deleteComposition = async (compositionID: number) => {
    if (!confirm('Are you sure you want to delete this composition?')) {
      return;
    }

    errorMessage = null;

    try {
      await deleteComposition(compositionID);

      compositions = compositions.filter(c => c.comp.id !== compositionID);
    } catch (err) {
      console.error('Failed to delete composition:', err);
      errorMessage = 'Failed to delete composition. Please try again.';
    }
  };

  $effect(() => {
    compositions = data.compositions;
  });
</script>

<svelte:head>
  <title>{data.user.username}'s Compositions</title>
</svelte:head>

<div class="root">
  <h2>{data.user.username}</h2>

  {#if errorMessage}
    <div class="error-message">{errorMessage}</div>
  {/if}

  <ul class="composition-list">
    {#each compositions as { comp, latest } (comp.id)}
      <li class="composition-item">
        <a href={`/geotoy/edit/${comp.id}`} tabindex="-1" class="thumbnail-link">
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
          <a href={`/geotoy/edit/${comp.id}`} class="title">{comp.title}</a>
          <div class="meta">
            <span class="date">last updated {new Date(comp.updated_at).toLocaleDateString()}</span>
            <span class="status" style={`color: ${comp.is_shared ? '#12cc12' : 'red'}`}>
              {comp.is_shared ? 'public' : 'private'}
            </span>
          </div>
        </div>

        {#if data.isMe}
          <div class="actions">
            <button class="delete-btn" onclick={() => deleteComposition(comp.id)}>delete</button>
          </div>
        {/if}
      </li>
    {/each}
    {#if compositions.length === 0}
      <li class="empty-message">{data.isMe ? 'you have no compositions' : 'no public compositions'}</li>
    {/if}
  </ul>

  <div class="back-link">
    <a href="/geotoy">home</a>
  </div>
</div>

<style>
  .root {
    max-width: 900px;
    margin: 0 auto;
    padding: 16px;
    height: 100vh;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  h2 {
    text-align: left;
    margin-bottom: 12px;
    font-size: 24px;
    font-weight: 400;
    border-bottom: 1px solid #555;
    padding-bottom: 4px;
  }

  .error-message {
    background-color: #401a1a;
    border: 1px solid #a04040;
    color: #f0c0c0;
    padding: 12px;
    margin-bottom: 16px;
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

  .title {
    font-size: 16px;
    font-weight: 600;
    color: #f0f0f0;
    text-decoration: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  a.title:hover {
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

  .back-link {
    margin-top: 32px;
    margin-bottom: 16px;
    text-align: center;
  }

  @media (max-width: 600px) {
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

    .title {
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
