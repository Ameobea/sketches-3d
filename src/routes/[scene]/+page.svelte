<script lang="ts">
  import { page } from '$app/stores';

  import Viz from '../../viz/Viz.svelte';

  $: sceneName = $page.params.scene.replace('.html', '').toLowerCase();
</script>

<Viz {sceneName} />
