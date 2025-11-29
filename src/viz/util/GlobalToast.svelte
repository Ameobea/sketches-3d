<script lang="ts">
  import { GlobalToastState, type ToastDescriptor } from './GlobalToastState.svelte';
  import Toast from './Toast.svelte';

  interface ActiveToast {
    id: number;
    toast: ToastDescriptor;
    startTime: number;
  }

  let toastIdCounter = 0;
  let activeToasts = $state<ActiveToast[]>([]);

  const removeToast = (id: number) => {
    activeToasts = activeToasts.filter(t => t.id !== id);
  };

  let lastDisplayedToast = $state(GlobalToastState.latestToast);
  $effect(() => {
    if (GlobalToastState.latestToast === lastDisplayedToast) {
      return;
    }

    const now = performance.now();
    const curToast = GlobalToastState.latestToast;
    lastDisplayedToast = curToast;
    if (!curToast) {
      return;
    }

    const id = toastIdCounter++;
    activeToasts = [...activeToasts, { id, toast: curToast, startTime: now }];

    setTimeout(() => removeToast(id), curToast.durationMs ?? 3000);
  });
</script>

{#if activeToasts.length > 0}
  <div class="toast-container">
    {#each activeToasts as { id, toast } (id)}
      <Toast {toast} onClose={() => removeToast(id)} />
    {/each}
  </div>
{/if}

<style lang="css">
  .toast-container {
    position: fixed;
    top: 16px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 9999;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }

  .toast-container > :global(*) {
    pointer-events: auto;
  }

  @media (max-width: 520px) {
    .toast-container {
      left: 16px;
      right: 16px;
      transform: none;
      align-items: stretch;
    }
  }
</style>
