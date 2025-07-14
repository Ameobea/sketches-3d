<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query';
  import type { AuthAPI } from './AuthAPI';

  export let onBack: () => void;
  export let API: AuthAPI;

  $: curUser = createQuery({
    queryKey: ['user'],
    queryFn: () => API.getPlayer(),
    refetchInterval: 25 * 60 * 1000,
  });

  $: isLoggedIn = !!$curUser.data;

  const logout = async () => {
    await API.logOutPlayer();

    username = '';
    password = '';
    passwordConfirm = '';

    API.setUserLoggedOut?.();
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
  let passwordConfirm = '';

  const submitLogin = async () => {
    if (loginMode === 'Register' && password !== passwordConfirm) {
      alert('Passwords do not match.');
      return;
    }

    try {
      if (loginMode === 'Login') {
        await API.login({ playerLogin: { username: username.toLowerCase(), password } });
      } else if (loginMode === 'Register') {
        await API.createPlayer({ playerLogin: { username: username.toLowerCase(), password } });
        await API.login({ playerLogin: { username: username.toLowerCase(), password } });
      }

      API.refetchUser();
      $curUser.refetch();

      username = '';
      password = '';
      passwordConfirm = '';
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

{#if $curUser.fetchStatus === 'fetching'}
  <div class="user-info">
    <span>Loading...</span>
  </div>
{:else if isLoggedIn}
  <div class="user-info">
    <span>Logged in as: {$curUser.data?.username ?? null}</span>
    <button class="menu-items-stack-item" style="font-size: 15px; padding: 2px 8px" on:click={logout}>
      Logout
    </button>
  </div>
{:else if loginMode}
  <div class="login-form" style:height={loginMode === 'Register' ? '267px' : '210px'}>
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
    {#if loginMode === 'Register'}
      <input
        class="menu-items-stack-item"
        type="password"
        placeholder="Confirm Password"
        bind:value={passwordConfirm}
        on:keypress={e => {
          if (e.key === 'Enter') {
            submitLogin();
          }
        }}
      />
    {/if}

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
    min-height: 90px;
    height: 100%;
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
    gap: 10px;
  }
</style>
