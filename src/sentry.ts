import { init, browserTracingIntegration, captureConsoleIntegration, captureMessage } from '@sentry/browser';

let sentryInitialized = false;
let sentryDisabled = false;

export const initSentry = () => {
  if (sentryInitialized) {
    return;
  }

  if (window.location.href.includes('localhost')) {
    sentryDisabled = true;
    return;
  }

  sentryInitialized = true;

  init({
    dsn: 'https://a437a29a32360db705c1fac14a714c70@sentry.ameo.design/15',
    integrations: [browserTracingIntegration(), captureConsoleIntegration({ levels: ['warn', 'error'] })],
    tracesSampleRate: 1.0,
  });
};

const sentryApi = { captureMessage };

export const getSentry = (): typeof sentryApi | null => {
  if (!sentryInitialized) {
    initSentry();
  }
  if (sentryDisabled) {
    return null;
  }
  return sentryApi;
};
