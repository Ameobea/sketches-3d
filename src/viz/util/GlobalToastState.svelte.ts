export interface ToastDescriptor {
  status: 'info' | 'success' | 'warning' | 'error';
  message: string;
  durationMs?: number;
}

const DefaultToastDuration = 2500;

export const GlobalToastState = $state<{ latestToast: ToastDescriptor | null }>({ latestToast: null });

export const showToast = (toast: ToastDescriptor) => {
  GlobalToastState.latestToast = { ...toast, durationMs: toast.durationMs ?? DefaultToastDuration };
};
