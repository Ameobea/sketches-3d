<script lang="ts">
  let { hasRun }: { hasRun: boolean } = $props();
</script>

<div class="placeholder">
  <span class="title">texture mode</span>
  <span class="sub">2D preview not implemented yet</span>
  <span class="state">{hasRun ? 'tree evaluated — outputs not yet readable from JS' : 'no run yet'}</span>
</div>

<style>
  .placeholder {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    background: #0d0d0d;
    user-select: none;
    pointer-events: none;
  }

  .title {
    font-size: 13px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: #0ff;
  }

  .sub {
    font-size: 11px;
    color: #8a8a8a;
  }

  .state {
    font-size: 9px;
    color: #5d5d5d;
  }
</style>
