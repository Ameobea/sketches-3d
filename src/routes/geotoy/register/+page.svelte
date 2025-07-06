<script lang="ts">
  import LoginRegisterForm from '../login/LoginRegisterForm.svelte';
</script>

<div class="root">
  <div class="login-container">
    <LoginRegisterForm mode="register" />
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
