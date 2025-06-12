<script lang="ts">
  import type { InfiniteConfig } from '../util';
  export let currentConfig: InfiniteConfig;
  export let time: number;
</script>

<div>
  <div class="time-display">
    <div class="score">Challenge Completed</div>
    {#if currentConfig.timerActive}
      <div class="time">
        <div>{(time / 1000).toFixed(3)}</div>
        Seconds
      </div>
    {/if}
  </div>
  <div class="mod-display">
    <div style="display: flex;flex-direction:column">
      <table>
        <thead>
          <tr style="border-style:dashed;border: 5px 5px 0px 0px;border-color:white">
            <th>Mod</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Seed</td>
            <td>"{currentConfig.seed}"</td>
          </tr>
          <tr>
            <td>Active</td>
            <td>{currentConfig.activePathLength}</td>
          </tr>
          <tr>
            <td>Goal</td>
            <td>{currentConfig.goalLength}</td>
          </tr>
          <tr>
            <td>VaryGap</td>
            <td>{currentConfig.varyingGaps}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</div>

<style lang="css">
  .time-display {
    position: absolute;
    top: 5%;
    left: 0;
    right: 0;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    background-color: #000000aa;
    color: white;
    border: 1px solid #ffffff55;
    padding: 10px;
    gap: 12px;
    min-width: 160px;
    max-width: 250px;
  }
  .mod-display {
    position: absolute;
    bottom: 0px;
    right: 0px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    background-color: #000000aa;
    color: white;
    border: 1px solid #ffffff55;
    padding: 10px;
    gap: 12px;
    min-width: 160px;
    max-width: 250px;
  }

  .time {
    display: flex;
    flex-direction: row;
    gap: 10px;
    font-size: 30px;
  }
</style>
