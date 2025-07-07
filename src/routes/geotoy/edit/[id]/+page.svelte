<script lang="ts">
  import Viz from 'src/viz/Viz.svelte';
  import type { PageData } from './$types';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import { page } from '$app/state';

  let { data }: { data: PageData } = $props();

  let renderMode = $derived(page.url.searchParams.get('render') === 'true');

  let userData: GeoscriptPlaygroundUserData = $derived({
    initialComposition: data,
    renderMode,
    me: data.me,
  });
</script>

<Viz sceneName="geoscript" {userData} />
