<script lang="ts">
  import { browser } from '$app/environment';
  import { createQuery } from '@tanstack/svelte-query';
  import { API, getUser, IsLoggedIn } from 'src/api/client';
  import { onMount } from 'svelte';

  export let mapID: string;
  export let userPlayID: string | null = null;
  export let userID: string | null = null;

  $: res = createQuery({
    queryKey: ['leaderboard', mapID, userPlayID],
    queryFn: async () => API.getLeaderboardByIndex({ mapId: mapID, startIndex: 0, endIndex: 20 }),
  });

  onMount(() => {
    if (browser) {
      getUser();
    }
  });
</script>

<div class="root">
  <h2>Leaderboard</h2>
  {#if $res.data}
    <div style="flex: 1">
      <table class="leaderboard-table">
        <thead>
          <tr>
            <th>Rank</th>
            <th>Player</th>
            <th>Time</th>
          </tr>
        </thead>
        <tbody>
          {#each $res.data as { playLength, playerUserName, id, playerId }, i}
            <tr
              class={(() => {
                if (id === userPlayID) {
                  return 'cur-play-highlight';
                } else if (playerId === userID) {
                  return 'cur-user-highlight';
                }

                return undefined;
              })()}
            >
              <td>{i + 1}</td>
              <td>{playerUserName}</td>
              <td>{playLength.toFixed(3)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if $res.error}
    <div style="color: red">Error fetching leaderboard: {$res.error.message}</div>
  {:else}
    <div>Loading...</div>
  {/if}
  {#if !$IsLoggedIn}
    <div style="color: #eb9b34; font-size: 12px;">
      <p>You must be logged in to save your times to the leaderboard</p>
      <p>You can log in via the pause menu (press ESC)</p>
    </div>
  {/if}
</div>

<style lang="css">
  .root {
    position: absolute;
    top: 20px;
    left: 20px;
    height: calc(100vh - 40px - 100px);
    width: 500px;
    display: flex;
    flex-direction: column;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    text-transform: uppercase;
    background-color: #000000aa;
    color: #eee;
    padding: 12px 12px 4px 12px;
  }

  h2 {
    text-align: center;
    margin-top: 2px;
    margin-bottom: 16px;
  }

  p {
    margin-top: 0px;
    margin-bottom: 4px;
  }

  table.leaderboard-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 14px;
  }

  th {
    text-align: left;
  }

  th,
  td {
    padding: 8px;
    border-bottom: 1px solid #777;
  }

  tr.cur-play-highlight {
    background-color: rgb(140, 41, 124);
  }

  tr.cur-user-highlight {
    color: rgb(208, 52, 235);
  }
</style>
