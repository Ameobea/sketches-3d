<script lang="ts">
  import type { ButtonSetting } from '../types';

  let { setting }: { setting: ButtonSetting } = $props();
</script>

<button class="button" type="button" onclick={setting.action}>{setting.label}</button>

<style>
  .button {
    width: 100%;
    height: var(--cp-row-h);
    border: none;
    font: inherit;
    text-align: center;
    background: var(--cp-bg2);
    color: var(--cp-text2);
    cursor: pointer;
  }
  .button:hover {
    background: var(--cp-bg2-hover);
  }
  .button:active {
    background: var(--cp-text2);
    color: var(--cp-bg2);
  }
</style>
