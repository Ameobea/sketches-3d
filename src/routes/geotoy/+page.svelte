<script lang="ts">
  import FrontPage from './FrontPage.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const description =
    'A Shadertoy-inspired tool for creating and sharing 3D scenes and models created using Geoscript - a DSL for generating and manipulating 3D meshes.';
</script>

<svelte:head>
  <title>Geotoy</title>
  <meta name="description" content={description} />
  <meta property="og:title" content="Geotoy" />
  <meta property="og:description" content={description} />
  <meta property="og:image" content="https://i.ameo.link/d5l.jpg" />
  <meta
    property="og:image:alt"
    content="A screenshot of the geotoy web UI.  It shows a 3D model with a gray rock-like texture composed of a central sphere with spokes pointing out of it and rings arranged around the outside connecting with the spokes.  On the bottom of the screen, there is a code editor with some geoscript code defining the scene.  There are also some stats displayed about the rendered scene (vertex count=36,578, total faces=35,472, etc.) and some minimal UI for interacting with the tool."
  />
</svelte:head>

<div class="root">
  <FrontPage
    featuredCompositions={data.featuredCompositions}
    me={data.me}
    currentPage={data.currentPage}
    hasMore={data.hasMore}
  />
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
