<script lang="ts">
  import 'src/index.css';
  import './geotoy.css';
  import GlobalToast from 'src/viz/util/GlobalToast.svelte';

  let { children } = $props();
</script>

<GlobalToast />
{@render children()}
