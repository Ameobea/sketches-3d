<script lang="ts">
  export let options: string[];
  export let curIx: number;
  export let onChange: (newIx: number) => void;
  export let loop: boolean = false;

  $: backDisabled = curIx === 0 && !loop;
  $: forwardDisabled = curIx === options.length - 1 && !loop;

  const selectPrev = () => {
    if (curIx === 0) {
      if (loop) {
        onChange(options.length - 1);
      }
    } else {
      onChange(curIx - 1);
    }
  };

  const selectNext = () => {
    if (curIx === options.length - 1) {
      if (loop) {
        onChange(0);
      }
    } else {
      onChange(curIx + 1);
    }
  };
</script>

<div class="root">
  <div
    class="arrow"
    style={backDisabled ? 'cursor: default; color: #777;' : undefined}
    role="button"
    tabindex="0"
    on:click={selectPrev}
  >
    <svg viewBox="0 0 24 24" width="24" height="24">
      <path fill="currentColor" d="M15.41 7.41L14 6l-6 6 6 6 1.41-1.41L10.83 12z" />
    </svg>
  </div>
  <div>{options[curIx]}</div>
  <div
    class="arrow"
    style={forwardDisabled ? 'cursor: default; color: #777;' : undefined}
    role="button"
    tabindex="0"
    on:click={selectNext}
  >
    <svg viewBox="0 0 24 24" width="24" height="24">
      <path fill="currentColor" d="M8.59 16.59L10 18l6-6-6-6-1.41 1.41L13.17 12z" />
    </svg>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    align-items: center;
  }

  .arrow {
    cursor: pointer;
  }
</style>
