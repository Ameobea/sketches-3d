const ENDPOINT = 'https://osu-api-bridge.ameo.dev/a/z';
const SALT = '4rW9XKHcEKa6bolWry8k0LGW';

interface AnalyticsEvent {
  category: string;
  subcategory: string;
  payload?: unknown;
}

const genSessionID = (): string => {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  return [...bytes].map(b => b.toString(16).padStart(2, '0')).join('');
};

let sessionID: string | null = null;
const getSessionID = (): string => {
  if (sessionID) {
    return sessionID;
  }
  try {
    sessionID = sessionStorage.getItem('analyticsSessionID');
    if (!sessionID) {
      sessionID = genSessionID();
      sessionStorage.setItem('analyticsSessionID', sessionID);
    }
  } catch (_err) {
    sessionID = genSessionID();
  }
  return sessionID;
};

const queues = new Map<string, AnalyticsEvent[]>();
let flushTimer: number | null = null;

const flushProject = async (project: string, events: AnalyticsEvent[]) => {
  try {
    const hashInput = new TextEncoder().encode(
      events.map(evt => evt.category + evt.subcategory).join('') + SALT
    );
    const digest = await crypto.subtle.digest('SHA-256', hashInput);
    const verification = [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, '0')).join('');

    await fetch(ENDPOINT, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ events, verification, project, session_id: getSessionID() }),
      keepalive: true,
    });
  } catch (_err) {
    // analytics must never break the app
  }
};

const flushAll = () => {
  for (const [project, events] of queues) {
    if (events.length) {
      void flushProject(project, events);
    }
  }
  queues.clear();
};

const logEvent = (project: string, category: string, subcategory: string, payload?: unknown) => {
  if (typeof window === 'undefined') {
    return;
  }
  if (window.location.href.includes('localhost')) {
    console.debug('[analytics]', project, category, subcategory, payload);
    return;
  }

  let queue = queues.get(project);
  if (!queue) {
    queue = [];
    queues.set(project, queue);
  }
  queue.push(payload === undefined ? { category, subcategory } : { category, subcategory, payload });
  if (flushTimer === null) {
    flushTimer = window.setTimeout(() => {
      flushTimer = null;
      flushAll();
    }, 800);
  }
};

export const logGeotoyEvent = (category: string, subcategory: string, payload?: unknown) =>
  logEvent('geotoy', category, subcategory, payload);

export const logDreamEvent = (category: string, subcategory: string, payload?: unknown) =>
  logEvent('dream', category, subcategory, payload);

if (typeof window !== 'undefined') {
  window.addEventListener('pagehide', flushAll);
}
