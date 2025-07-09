<div class="root">
  <h2>Geotoy Docs + Reference</h2>

  <p>
    Geotoy is a tool for creating and sharing 3D scenes and models created using <b>Geoscript</b>
    - a DSL for generating and manipulating 3D meshes.
  </p>
  <p>
    It's inspired heavily by <a href="https://shadertoy.com">Shadertoy</a>
    , but focuses on geometry rather than on shaders.
  </p>

  <ul>
    <li><a href="/geoscript/docs/quickstart">Geoscript Language Quickstart</a></li>
    <li><a href="/geoscript/docs">Geoscript Builtin Function Reference</a></li>
  </ul>

  <div style="text-align: center; margin-top: 2em; font-size: 0.9em; color: #888">
    <a href="/geotoy">back to home</a>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 16px;
    max-width: 1000px;
    margin: auto;
    line-height: 2;
  }

  p {
    line-height: 1.4;
  }

  h2 {
    margin-bottom: 32px;
  }

  @media (max-width: 600px) {
    ul {
      padding-inline-start: 20px;
    }

    h2 {
      font-size: 22px;
      font-weight: bold;
      margin-bottom: 24px;
    }
  }
</style>
