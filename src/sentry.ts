import * as Sentry from '@sentry/browser';

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

  Sentry.init({
    dsn: 'https://a437a29a32360db705c1fac14a714c70@sentry.ameo.design/15',
    integrations: [
      Sentry.browserTracingIntegration(),
      Sentry.captureConsoleIntegration({ levels: ['warn', 'error'] }),
    ],
    tracesSampleRate: 1.0,
  });
};

export const getSentry = (): typeof Sentry | null => {
  if (!sentryInitialized) {
    initSentry();
  }
  if (sentryDisabled) {
    return null;
  }
  return Sentry;
};
