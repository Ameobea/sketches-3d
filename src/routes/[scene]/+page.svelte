<script lang="ts">
  import Viz from '../../viz/Viz.svelte';
  import type { PageData } from './$types';

  export let data: PageData;
</script>

<Viz sceneName={data.sceneName} userData={data.levelDef ?? undefined} />
