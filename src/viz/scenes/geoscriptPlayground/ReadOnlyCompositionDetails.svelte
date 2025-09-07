<script lang="ts">
  import type { Composition, CompositionVersion } from 'src/geoscript/geotoyAPIClient';

  let { comp, version }: { comp: Composition; version: CompositionVersion } = $props();
</script>

<div class="root">
  <div class="form-grid">
    <label class="label" for="composition-details-title">Title:</label>
    <div class="value" id="composition-details-title">{comp.title}</div>
    <label class="label" for="composition-details-author">Author:</label>
    <div class="value" id="composition-details-author">
      <a href={`/geotoy/user/${comp.author_id}`}>{comp.author_username}</a>
    </div>
    <label class="label" for="composition-details-description">Description:</label>
    <div class="value description" id="composition-details-description">{comp.description}</div>
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

    > * {
      font-size: 14px;
    }

    .label {
      color: #999;
      text-align: right;
    }

    .value {
      white-space: pre-wrap;
    }

    .description {
      max-height: 6em;
      overflow: auto;
    }
  }
</style>
