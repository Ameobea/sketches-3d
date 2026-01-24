<script lang="ts">
  import { afterNavigate } from '$app/navigation';
  import LoginRegisterForm from '../login/LoginRegisterForm.svelte';

  const RETURN_TO_STORAGE_KEY = 'geotoy:returnTo';
  let returnTo: string | null = null;

  if (typeof sessionStorage !== 'undefined') {
    returnTo = sessionStorage.getItem(RETURN_TO_STORAGE_KEY);
  }

  afterNavigate(({ from }) => {
    if (!from?.url) {
      return;
    }
    if (from.url.pathname === '/geotoy/login' || from.url.pathname === '/geotoy/register') {
      return;
    }
    const next = `${from.url.pathname}${from.url.search}${from.url.hash}`;
    returnTo = next;
    if (typeof sessionStorage !== 'undefined') {
      sessionStorage.setItem(RETURN_TO_STORAGE_KEY, next);
    }
  });
</script>

<div class="root">
  <div class="login-container">
    <LoginRegisterForm mode="register" {returnTo} />
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
