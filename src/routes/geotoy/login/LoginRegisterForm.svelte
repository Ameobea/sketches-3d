<script lang="ts">
  import { login, register } from 'src/geoscript/geotoyAPIClient';

  export let mode: 'login' | 'register';
  export let returnTo: string | null = null;

  let username = '';
  let password = '';
  let confirmPassword = '';

  let loginError: string | null = null;

  const redirectAfterAuth = () => {
    const fallback = '/geotoy';
    const storageKey = 'geotoy:returnTo';
    const sanitizeTarget = (rawTarget: string | null): string | null => {
      if (!rawTarget) {
        return null;
      }
      try {
        const targetUrl = new URL(rawTarget, window.location.origin);
        if (targetUrl.origin !== window.location.origin) {
          return null;
        }
        if (targetUrl.pathname === '/geotoy/login' || targetUrl.pathname === '/geotoy/register') {
          return null;
        }
        return `${targetUrl.pathname}${targetUrl.search}${targetUrl.hash}`;
      } catch {
        return null;
      }
    };

    const storedReturnTo =
      typeof sessionStorage !== 'undefined' ? sessionStorage.getItem(storageKey) : null;
    const target =
      sanitizeTarget(returnTo) ?? sanitizeTarget(storedReturnTo) ?? sanitizeTarget(document.referrer);

    if (typeof sessionStorage !== 'undefined') {
      sessionStorage.removeItem(storageKey);
    }
    window.location.href = target ?? fallback;
  };
</script>

<div class="root">
  <div class="login-container">
    <h2>{mode}</h2>
    <form
      on:submit|preventDefault={async () => {
        if (mode === 'register' && password !== confirmPassword) {
          loginError = 'passwords do not match';
          return;
        }

        try {
          await (mode === 'login' ? login : register)({ username, password });
          if (mode === 'register') {
            await login({ username, password });
          }
        } catch (error) {
          loginError = error instanceof Error ? error.message : `unknown error: ${error}`;
          return;
        }
        redirectAfterAuth();
      }}
    >
      <label class="form-row">
        <span class="form-label">username</span>
        <input type="text" name="username" required bind:value={username} class="form-input" />
      </label>
      <label class="form-row">
        <span class="form-label">password</span>
        <input type="password" name="password" required bind:value={password} class="form-input" />
      </label>
      {#if mode === 'register'}
        <label class="form-row">
          <span class="form-label">confirm password</span>
          <input
            type="password"
            name="confirm_password"
            required
            bind:value={confirmPassword}
            class="form-input"
          />
        </label>
      {/if}
      <button type="submit">{mode}</button>
    </form>

    {#if loginError}
      <p class="error">{loginError}</p>
    {/if}
    <p style="text-align: center; font-size: 12px;">
      {#if mode === 'login'}
        <a href="/geotoy/register">register new account</a>
      {:else}
        <a href="/geotoy/login">login to existing account</a>
      {/if}
    </p>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  h2 {
    margin-top: -2px;
    margin-bottom: 20px;
    font-size: 24px;
  }

  .login-container {
    padding: 8px 16px;
    border: 1px solid #2f2f2f;
    margin: auto;
    width: 400px;

    form {
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
  }

  .form-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;

    input {
      width: 200px;
    }
  }

  .form-label {
    min-width: 136px;
    text-align: left;
    font-size: 14px;
  }

  .form-input {
    flex: 1 1 200px;
    max-width: 250px;
  }

  .error {
    color: red;
    font-size: 12px;
    margin-top: 8px;
  }

  button {
    margin-top: 8px;
    height: 28px;
  }

  p {
    margin-top: 12px;
    margin-bottom: 0;
  }

  @media (max-width: 600px) {
    .login-container {
      width: 98%;
      box-sizing: border-box;
    }

    .form-label {
      min-width: 120px;
      font-size: 12px;
    }

    .form-input {
      flex: 1 1 120px;
      max-width: 150px;
    }
  }
</style>
