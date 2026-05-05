<script lang="ts">
  import Viz from '../viz/Viz.svelte';
  import { processLoadedScene } from '../viz/scenes/nexus/nexus';
  import { SCENE_REGISTRY } from '../viz/scenes/sceneRegistry';
  import type { SceneDef } from '../viz/scenes';

  const { modulePath: _modulePath, ...data } = SCENE_REGISTRY['nexus'];
  const sceneDef: SceneDef = { ...data, sceneLoader: () => processLoadedScene };
</script>

<Viz sceneName="nexus" {sceneDef} />
