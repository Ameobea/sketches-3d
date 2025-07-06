<script lang="ts">
  import LoginRegisterForm from './LoginRegisterForm.svelte';
</script>

<div class="root">
  <div class="login-container">
    <LoginRegisterForm mode="login" />
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
