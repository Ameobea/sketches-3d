import * as Sentry from '@sentry/browser';
import { Integrations } from '@sentry/tracing';
import { CaptureConsole } from '@sentry/integrations';

let sentryInitialized = false;
let sentryDisabled = false;

export const initSentry = () => {
  if (sentryInitialized || window.location.href.includes('localhost')) {
    sentryDisabled = true;
    return;
  }

  sentryInitialized = true;

  Sentry.init({
    dsn: 'https://78a1bcc6a9fc40568e135c0ff991f526@sentry.ameo.design/11',
    integrations: [
      new Integrations.BrowserTracing(),
      new CaptureConsole({ levels: ['warn', 'error'] }),
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
