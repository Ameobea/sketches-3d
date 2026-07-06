<script lang="ts">
  let { value, onChange }: { value: string; onChange: (v: string) => void } = $props();
</script>

<input
  class="text"
  type="text"
  value={value ?? ''}
  oninput={e => onChange((e.currentTarget as HTMLInputElement).value)}
/>

<style>
  .text {
    width: 100%;
    height: var(--cp-row-h);
    padding: 0 6px;
    border: none;
    outline: none;
    font: inherit;
    background: var(--cp-bg2);
    color: var(--cp-text2);
  }
</style>
