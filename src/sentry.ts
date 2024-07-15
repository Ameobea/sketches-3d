import * as Sentry from '@sentry/browser';

let sentryInitialized = false;
let sentryDisabled = false;

export const initSentry = () => {
  if (sentryInitialized || window.location.href.includes('localhost')) {
    sentryDisabled = true;
    return;
  }

  sentryInitialized = true;

  Sentry.init({
    dsn: 'https://e71a66fc87db4733bc42b675e6f9bc78@sentry.ameo.design/14',
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
