<script lang="ts">
  export let curTime: number | null;

  $: [digits, decimals] = (curTime ?? 0).toFixed(3).split('.');
</script>

<span class="root">
  {#if curTime !== null}
    <span class="digits">{digits}</span>
    <span class="point">.</span>
    <span class="decimals">{decimals}</span>
  {/if}
</span>

<style lang="css">
  .root {
    background-color: #00000066;
    padding: 4px;
    position: absolute;
    bottom: 63px;
    left: 20px;
    font-size: 30px;
    color: #fcfcfc;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    pointer-events: none;
  }

  .digits {
    font-size: 40px;
    margin-right: -19px;
  }

  .point {
    font-size: 30px;
    letter-spacing: -5px;
  }

  .decimals {
    margin-left: -14px;
    font-size: 20px;
    color: #aaa;
  }
</style>
