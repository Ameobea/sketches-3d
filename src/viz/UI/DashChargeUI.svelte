<script lang="ts">
  export let curCharges: number;
</script>

<!-- TODO: Should display icons rather than a number -->
<div class="root">
  Dashes: <b>{curCharges === Infinity ? '∞' : curCharges}</b>
</div>

<style lang="css">
  .root {
    background-color: #00000066;
    padding: 4px;
    position: absolute;
    bottom: 20px;
    left: 20px;
    font-size: 30px;
    color: #fcfcfc;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    pointer-events: none;
    user-select: none;
  }
</style>
