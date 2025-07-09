<script lang="ts">
  import SvelteSeo from 'svelte-seo';

  import FrontPage from './FrontPage.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const description =
    'A Shadertoy-inspired tool for creating and sharing 3D scenes and models created using Geoscript - a DSL for generating and manipulating 3D meshes.';
</script>

<SvelteSeo
  title="Geotoy"
  {description}
  openGraph={{
    title: 'Geotoy',
    description,
    images: [
      {
        url: 'https://i.ameo.link/d5l.jpg',
        alt: 'A screenshot of the geotoy web UI.  It shows a 3D model with a gray rock-like texture composed of a central sphere with spokes pointing out of it and rings arranged around the outside connecting with the spokes.  On the bottom of the screen, there is a code editor with some geoscript code defining the scene.  There are also some stats displayed about the rendered scene (vertex count=36,578, total faces=35,472, etc.) and some minimal UI for interacting with the tool.',
        width: 1728,
        height: 1226,
      },
    ],
  }}
/>

<div class="root">
  <FrontPage featuredCompositions={data.featuredCompositions} me={data.me} />
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
