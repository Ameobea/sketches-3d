<script lang="ts">
  let { value, onChange }: { value: boolean; onChange: (v: boolean) => void } = $props();
</script>

<label class="checkbox" class:checked={!!value}>
  <input
    type="checkbox"
    checked={!!value}
    onchange={e => onChange((e.currentTarget as HTMLInputElement).checked)}
  />
</label>

<style>
  .checkbox {
    display: inline-block;
    width: 18px;
    height: 18px;
    background: var(--cp-bg2);
    cursor: pointer;
  }

  .checkbox input {
    display: none;
  }

  .checkbox.checked::after {
    content: '';
    display: block;
    width: 10px;
    height: 10px;
    margin: 4px;
    background: var(--cp-fg1);
  }
</style>
