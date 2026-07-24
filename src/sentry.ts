import { init, browserTracingIntegration, captureConsoleIntegration } from '@sentry/browser';

let sentryInitialized = false;

export const initSentry = () => {
  if (sentryInitialized || window.location.href.includes('localhost')) {
    return;
  }

  sentryInitialized = true;

  init({
    dsn: 'https://a437a29a32360db705c1fac14a714c70@sentry.ameo.design/15',
    integrations: [browserTracingIntegration(), captureConsoleIntegration({ levels: ['warn', 'error'] })],
    tracesSampleRate: 1.0,
  });
};
