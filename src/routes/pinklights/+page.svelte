<script lang="ts">
  import Viz from '../../viz/Viz.svelte';
  import { processLoadedScene } from '../../viz/scenes/chasms/chasms';
  import { SCENE_REGISTRY } from '../../viz/scenes/sceneRegistry';
  import type { SceneDef } from '../../viz/scenes';

  const { modulePath: _modulePath, ...data } = SCENE_REGISTRY['chasms'];
  const sceneDef: SceneDef = { ...data, sceneLoader: () => processLoadedScene };
</script>

<Viz sceneName="chasms" {sceneDef} />
