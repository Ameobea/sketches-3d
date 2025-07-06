<script lang="ts">
  import Viz from 'src/viz/Viz.svelte';
  import type { PageData } from './$types';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground';
  import { page } from '$app/state';

  let { data }: { data: PageData } = $props();

  let renderMode = $derived(page.url.searchParams.get('render') === 'true');

  let userData: GeoscriptPlaygroundUserData = $derived({
    initialComposition: { comp: data.comp, version: data.latest },
    renderMode,
  });
</script>

<Viz sceneName="geoscript" {userData} />
