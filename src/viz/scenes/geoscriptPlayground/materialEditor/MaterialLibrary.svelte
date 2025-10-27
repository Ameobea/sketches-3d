<script lang="ts">
  import { listMaterials } from 'src/geoscript/geotoyAPIClient';
  import type { MaterialDescriptor } from 'src/geoscript/materials';
  import ItemPicker from './ItemPicker.svelte';

  let {
    onclose = () => {},
    onselect = (_: MaterialDescriptor | null) => {},
  }: {
    onclose: () => void;
    onselect: (material: MaterialDescriptor | null) => void;
  } = $props();

  let materials = $state<MaterialDescriptor[]>([]);
  let isLoading = $state(true);
  let selectedId = $state<number | null>(null);

  $effect(() => {
    isLoading = true;
    listMaterials().then(materialList => {
      materials = materialList;
      isLoading = false;
    });
  });

  const handleSubmit = () => {
    if (selectedId === null) {
      return;
    }
    const selectedMaterial = materials.find(m => m.id === selectedId);
    if (selectedMaterial) {
      onselect(selectedMaterial);
    }
    onclose();
  };

  const handleSelect = (id: number | null | string) => {
    if (typeof id === 'string') {
      throw new Error('unreachable; id should not be a string');
    }
    selectedId = id;
  };
</script>

{#if isLoading}
  <div class="loading">loading...</div>
{:else}
  <ItemPicker
    title="Select Material"
    {selectedId}
    onselect={handleSelect}
    items={materials.map(m => ({ ...m, url: m.thumbnailUrl }))}
    {onclose}
    showNoneOption={false}
  >
    <div slot="footer-end">
      <button class="footer-button" onclick={onclose}>cancel</button>
      <button class="footer-button" onclick={handleSubmit} disabled={selectedId === null}>import</button>
    </div>
  </ItemPicker>
{/if}

<style>
  .loading {
    font-size: 14px;
    text-align: center;
    padding: 16px;
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
  }
</style>
