import { config } from './conf';

// Liveness of the upstream Google Fonts webfonts API for the configured key.
// The svg-text-to-path library fetches this list internally and throws a cryptic
// `data.items.forEach` TypeError when the key is expired/invalid/quota'd; we probe
// the same endpoint so failures surface with Google's actual reason.

export interface FontApiHealth {
  ok: boolean;
  reason?: string;
  message?: string;
  checkedAt: number;
}

const PROBE_URL = 'https://www.googleapis.com/webfonts/v1/webfonts?sort=popularity&key=';
const MIN_PROBE_INTERVAL_MS = 60_000;

let last: FontApiHealth | null = null;
let inflight: Promise<FontApiHealth> | null = null;

async function runProbe(): Promise<FontApiHealth> {
  const now = Date.now();
  if (!config.googleFontsApiKey) {
    return { ok: false, reason: 'NO_API_KEY', message: 'GOOGLE_FONTS_API_KEY is not set', checkedAt: now };
  }
  try {
    const res = await fetch(PROBE_URL + encodeURIComponent(config.googleFontsApiKey));
    const data: any = await res.json().catch(() => null);
    if (data && Array.isArray(data.items)) {
      return { ok: true, checkedAt: now };
    }
    const err = data?.error;
    const reason =
      err?.details?.find?.((d: any) => d?.reason)?.reason ?? err?.errors?.[0]?.reason ?? err?.status ?? `HTTP_${res.status}`;
    return {
      ok: false,
      reason,
      message: err?.message ?? `Unexpected webfonts response (HTTP ${res.status})`,
      checkedAt: now,
    };
  } catch (e) {
    return { ok: false, reason: 'FETCH_FAILED', message: e instanceof Error ? e.message : String(e), checkedAt: now };
  }
}

export async function checkFontApi(force = false): Promise<FontApiHealth> {
  if (!force && last && Date.now() - last.checkedAt < MIN_PROBE_INTERVAL_MS) {
    return last;
  }
  if (!inflight) {
    inflight = runProbe().finally(() => {
      inflight = null;
    });
  }
  const result = await inflight;
  last = result;
  return result;
}

export function lastFontApiHealth(): FontApiHealth | null {
  return last;
}

function logHealth(h: FontApiHealth): void {
  if (h.ok) {
    console.log('Google Fonts API OK');
  } else {
    console.error(
      `\n!!! GOOGLE FONTS API UNHEALTHY [${h.reason}]: ${h.message}\n` +
        `    Text-to-path conversion will FAIL for uncached requests until this is resolved.\n` +
        `    If the key expired/was revoked, renew it and redeploy with a fresh GOOGLE_FONTS_API_KEY.\n`
    );
  }
}

// Probe at boot and periodically; log the initial state and any later transition
// so an expired/revoked key shows up in `docker logs` within minutes.
export function startFontApiMonitor(intervalMs = 600_000): void {
  checkFontApi(true).then(logHealth);
  const timer = setInterval(() => {
    const prev = last?.ok;
    checkFontApi(true).then(h => {
      if (h.ok !== prev) logHealth(h);
    });
  }, intervalMs);
  timer.unref?.();
}
