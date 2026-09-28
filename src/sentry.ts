import { init, browserTracingIntegration, captureConsoleIntegration } from '@sentry/browser';

const isLocalDev = () => /^(localhost|127\.0\.0\.1|\[::1\])$/.test(window.location.hostname);

let sentryInitialized = false;

export const initSentry = () => {
  if (sentryInitialized || isLocalDev()) {
    return;
  }

  sentryInitialized = true;

  init({
    dsn: 'https://a437a29a32360db705c1fac14a714c70@sentry.ameo.design/15',
    integrations: [browserTracingIntegration(), captureConsoleIntegration({ levels: ['warn', 'error'] })],
    tracesSampleRate: 1.0,
  });
};
