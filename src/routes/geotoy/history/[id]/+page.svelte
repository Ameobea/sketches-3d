<script lang="ts">
  import { resolve } from '$app/paths';

  import GeotoyHeader from '../../GeotoyHeader.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };
</script>

<svelte:head>
  <title>Revision History: {data.composition.title}</title>
</svelte:head>

<div class="root">
  <GeotoyHeader me={data.me} showTitleLink />

  <div class="content">
    <div class="header-section">
      <h2>revision history: {data.composition.title}</h2>
      <div class="meta-info">
        <span>
          by <a href={resolve(`/geotoy/user/${data.composition.author_id}`)}>
            {data.composition.author_username}
          </a>
        </span>
        <span class="separator">•</span>
        <a href={resolve(`/geotoy/edit/${data.composition.id}`)}>view latest</a>
      </div>
    </div>

    <ul class="version-list">
      {#each data.versions as version, index (version.id)}
        {@const isLatest = index === 0}
        <li class="version-item">
          <a
            href={resolve(`/geotoy/edit/${data.composition.id}?version_id=${version.id}`)}
            tabindex="-1"
            class="thumbnail-link"
          >
            {#if version.thumbnail_url}
              <img
                src={version.thumbnail_url}
                alt="Version {version.id} thumbnail"
                class="thumbnail"
                crossorigin="anonymous"
                loading="lazy"
                width="140"
                height="140"
              />
            {:else}
              <div class="thumbnail placeholder"></div>
            {/if}
          </a>

          <div class="info">
            <a
              href={resolve(`/geotoy/edit/${data.composition.id}?version_id=${version.id}`)}
              class="version-link"
            >
              {version.id}
              {#if isLatest}
                <span class="latest-badge">latest</span>
              {/if}
            </a>
            <div class="version-meta">
              <span class="date">{formatDate(version.created_at)}</span>
            </div>
          </div>
        </li>
      {/each}
      {#if data.versions.length === 0}
        <li class="empty-message">No version history available</li>
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

  .header-section {
    margin-bottom: 16px;
    border-bottom: 1px solid #555;
    padding-bottom: 8px;
  }

  h2 {
    text-align: left;
    margin: 0 0 8px 0;
    font-size: 24px;
    font-weight: 400;
  }

  .meta-info {
    font-size: 14px;
    color: #aaa;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .meta-info a {
    color: #7cb3f0;
    text-decoration: none;
  }

  .meta-info a:hover {
    text-decoration: underline;
  }

  .separator {
    color: #555;
  }

  .version-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .version-item {
    display: flex;
    align-items: flex-start;
    gap: 20px;
    padding: 12px 0;
    border-bottom: 1px solid #2a2a2a;
  }

  .version-item:last-child {
    border-bottom: none;
  }

  .thumbnail-link {
    display: block;
    flex-shrink: 0;
  }

  .thumbnail {
    width: 140px;
    height: 140px;
    object-fit: cover;
    display: block;
    background: #222;
  }

  .placeholder {
    width: 140px;
    height: 140px;
    background: #222
      repeating-linear-gradient(-45deg, transparent, transparent 9px, #181818 9px, #181818 18px);
  }

  .info {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .version-link {
    font-weight: 400;
    font-size: 18px;
    font-weight: 600;
    color: #f0f0f0;
    text-decoration: none;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .version-link:hover {
    color: #fff;
    text-decoration: underline;
  }

  .latest-badge {
    font-size: 11px;
    font-weight: 400;
    background: #2a6e2a;
    color: #90ee90;
    padding: 2px 6px;
    border-radius: 3px;
  }

  .version-meta {
    font-size: 13px;
    color: #aaa;
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
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

    .meta-info {
      font-size: 12px;
      flex-wrap: wrap;
    }

    .version-item {
      gap: 14px;
    }

    .thumbnail,
    .placeholder {
      width: 100px;
      height: 100px;
    }

    .version-link {
      font-size: 16px;
      flex-wrap: wrap;
      gap: 6px;
    }

    .version-meta {
      font-size: 12px;
      gap: 12px;
    }
  }
</style>
