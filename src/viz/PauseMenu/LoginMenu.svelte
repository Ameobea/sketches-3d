<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query';
  import { ResponseError } from 'src/api';
  import { API } from 'src/api/client';

  export let onBack: () => void;

  $: curUser = createQuery({
    queryKey: ['user'],
    queryFn: async () => {
      try {
        return await API.getPlayer();
      } catch (err) {
        if (err instanceof ResponseError) {
          if (err.response.status === 401) {
            return null;
          }
        }

        console.error('Error fetching user data:', err);
        throw err;
      }
    },
    refetchInterval: 25 * 60 * 1000,
  });

  $: isLoggedIn = !!$curUser.data;

  const logout = async () => {
    await API.logOutPlayer();
    $curUser.refetch();
  };

  let loginMode: 'Login' | 'Register' | null = null;
  const handleLoginClick = () => {
    loginMode = 'Login';
  };

  const handleRegisterClick = () => {
    loginMode = 'Register';
  };

  let username = '';
  let password = '';

  const submitLogin = async () => {
    try {
      if (loginMode === 'Login') {
        await API.login({ playerLogin: { username: username.toLowerCase(), password } });
      } else if (loginMode === 'Register') {
        await API.createPlayer({ playerLogin: { username: username.toLowerCase(), password } });
        await API.login({ playerLogin: { username: username.toLowerCase(), password } });
      }
      $curUser.refetch();
    } catch (error) {
      console.error('Error during login/register:', error);
      alert(
        loginMode === 'Login'
          ? 'Login failed. Invalid username or password?'
          : 'Registration failed. Try another username.'
      );
    }
  };
</script>

{#if isLoggedIn}
  <div class="user-info">
    <span>Logged in as: {$curUser.data?.username}</span>
    <button class="menu-items-stack-item" style="font-size: 15px; padding: 2px 8px" on:click={logout}>
      Logout
    </button>
  </div>
{:else if loginMode}
  <div class="login-form">
    <h3 style="margin-bottom: -4px;">{loginMode}</h3>
    <input
      class="menu-items-stack-item"
      type="text"
      placeholder="Username"
      bind:value={username}
      on:keypress={e => {
        if (e.key === 'Enter') {
          submitLogin();
        }
      }}
    />
    <input
      class="menu-items-stack-item"
      type="password"
      placeholder="Password"
      bind:value={password}
      on:keypress={e => {
        if (e.key === 'Enter') {
          submitLogin();
        }
      }}
    />

    <div class="side-by-side-buttons">
      <button class="menu-items-stack-item" on:click={() => (loginMode = null)} style="color: #ccc">
        Cancel
      </button>
      <button class="menu-items-stack-item" disabled={!username || !password} on:click={submitLogin}>
        Submit
      </button>
    </div>
  </div>
{:else}
  <div class="side-by-side-buttons">
    <button class="menu-items-stack-item" on:click={handleLoginClick}>Login</button>
    <button class="menu-items-stack-item" on:click={handleRegisterClick}>Register</button>
  </div>
{/if}
<button on:click={onBack}>Back</button>

<style lang="css">
  .user-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    height: 90px;
  }

  .side-by-side-buttons {
    display: flex;
    flex: 1;
    gap: 10px;
    height: 82px;
    align-items: center;
    justify-content: space-between;

    > button {
      width: 190px;
    }
  }

  .side-by-side-buttons,
  .login-form {
    padding: 0;
    border: none;
  }

  .login-form {
    display: flex;
    flex-direction: column;
    height: 210px;
    gap: 10px;
  }
</style>
