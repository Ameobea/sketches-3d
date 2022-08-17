<script lang="ts">
  import { page } from '$app/stores';

  import { initViz } from '../../viz';
  import '../index.css';

  $: sceneName = $page.params.scene;
</script>

<div use:initViz={sceneName} />
