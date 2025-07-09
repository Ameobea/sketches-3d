<script lang="ts">
  import Viz from 'src/viz/Viz.svelte';
  import type { PageData } from './$types';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';

  let { data }: { data: PageData } = $props();
  let userData: GeoscriptPlaygroundUserData = $derived({
    me: data.me,
    renderMode: false,
    initialComposition: null,
  });
</script>

<Viz sceneName="geoscript" {userData} />
