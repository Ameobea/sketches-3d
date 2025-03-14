<script lang="ts" context="module">
  export enum Score {
    SPlus = 0,
    S = 1,
    A = 2,
    B = 3,
    C = 4,
  }

  export type ScoreThresholds = { [K in Exclude<Score, Score.C>]: number };

  const ScoreNames: { [key in Score]: string } = {
    [Score.SPlus]: 'S+',
    [Score.S]: 'S',
    [Score.A]: 'A',
    [Score.B]: 'B',
    [Score.C]: 'C',
  };

  const ScoreColors: { [key in Score]: string } = {
    [Score.SPlus]: '#f01fff',
    [Score.S]: '#ffd700',
    [Score.A]: '#18e609',
    [Score.B]: '#0b74e3',
    [Score.C]: '#bd800f',
  };

  const ScoreFontSizes: { [key in Score]: string } = {
    [Score.SPlus]: '60px',
    [Score.S]: '50px',
    [Score.A]: '40px',
    [Score.B]: '40px',
    [Score.C]: '40px',
  };
</script>

<script lang="ts">
  import { QueryClientProvider } from '@tanstack/svelte-query';

  import MiniLeaderboardDisplay from './MiniLeaderboardDisplay.svelte';
  import { queryClient } from '../queryClient';

  export let mapID: string;
  export let scoreThresholds: ScoreThresholds;
  export let time: number;
  export let userPlayID: string | null = null;
  export let userID: string | null = null;
  // TODO: Highlight the current score if it's in the list

  $: score = (() => {
    if (time < scoreThresholds[Score.SPlus]) return Score.SPlus;
    if (time < scoreThresholds[Score.S]) return Score.S;
    if (time < scoreThresholds[Score.A]) return Score.A;
    if (time < scoreThresholds[Score.B]) return Score.B;
    return Score.C;
  })();
  $: scoreFontSize = ScoreFontSizes[score];
</script>

<div class="time-display">
  <div class="score" style="color: {ScoreColors[score]}; font-size: {scoreFontSize}">
    {ScoreNames[score]}
  </div>
  <div class="time">
    <div style={score <= Score.S ? 'font-weight: bold' : undefined}>{time.toFixed(3)}</div>
    Seconds
  </div>
</div>

<QueryClientProvider client={queryClient}>
  <MiniLeaderboardDisplay {mapID} {userPlayID} {userID} />
</QueryClientProvider>

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
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
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
