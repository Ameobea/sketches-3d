<script lang="ts">
  import type { ToastDescriptor } from './GlobalToastState.svelte';

  let { toast, onClose }: { toast: ToastDescriptor; onClose: () => void } = $props();

  const statusLabels: Record<ToastDescriptor['status'], string> = {
    info: 'INFO',
    success: 'OK',
    warning: 'WARN',
    error: 'ERR',
  };
</script>

<div class="toast {toast.status}">
  <div class="content">
    <span class="status-label">[{statusLabels[toast.status]}]</span>
    <span class="message">{toast.message}</span>
  </div>
  <button class="close-btn" onclick={onClose} aria-label="Dismiss toast">×</button>
</div>

<style lang="css">
  .toast {
    position: relative;
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
    padding: 12px 32px 12px 12px;
    border: 2px solid;
    background: #1a1a1a;
    color: #e0e0e0;
    font-family: monospace;
    font-size: 14px;
    width: max-content;
    max-width: min(480px, calc(100vw - 32px));
    box-sizing: border-box;
  }

  .toast.info {
    border-color: #888;
  }

  .toast.success {
    border-color: #4a4;
  }

  .toast.warning {
    border-color: #ca0;
  }

  .toast.error {
    border-color: #c44;
  }

  .content {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
    flex: 1;
    min-width: 0;
  }

  .status-label {
    flex-shrink: 0;
    font-weight: bold;
  }

  .toast.info .status-label {
    color: #aaa;
  }

  .toast.success .status-label {
    color: #6c6;
  }

  .toast.warning .status-label {
    color: #ec2;
  }

  .toast.error .status-label {
    color: #e66;
  }

  .message {
    flex: 1;
    word-break: break-word;
  }

  .close-btn {
    position: absolute;
    top: 6px;
    right: 6px;
    background: none;
    border: none;
    color: #666;
    font-size: 24px;
    line-height: 1;
    cursor: pointer;
    font-family: monospace;
    margin-right: -8px;
  }

  .close-btn:hover {
    color: #e0e0e0;
  }
</style>
