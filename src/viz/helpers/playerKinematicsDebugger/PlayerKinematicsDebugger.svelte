<script lang="ts">
  export let verticalVelocity: number;
  export let isJumping: boolean;
  export let isDashing: boolean;
  export let isOnGround: boolean;

  $: formattedVerticalVelocity = (() => {
    let formatted = verticalVelocity.toFixed(2);
    while (formatted.length < 6) {
      formatted = ' ' + formatted;
    }
    return formatted;
  })();

  const getBoolColor = (bool: boolean) => (bool ? 'green' : 'red');

  const formatBool = (bool: boolean) => (bool ? ' true' : 'false');
</script>

<div class="root">
  <div>
    m_verticalVelocity: <pre>{formattedVerticalVelocity}</pre>
  </div>
  <div>
    isJumping: <pre style="color: {getBoolColor(isJumping)}"> {formatBool(isJumping)}</pre>
  </div>
  <div>
    isDashing: <pre style="color: {getBoolColor(isDashing)}"> {formatBool(isDashing)}</pre>
  </div>
  <div>
    isOnGround: <pre style="color: {getBoolColor(isOnGround)}"> {formatBool(isOnGround)}</pre>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    text-align: right;
  }

  .root > div {
    display: flex;
    flex-direction: row;
    justify-content: flex-end;
  }

  pre {
    margin: 0;
    padding: 0;
  }
</style>
