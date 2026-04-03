import { existsSync, readFileSync, watch } from 'fs';
import { join } from 'path';

import type { RequestHandler } from '@sveltejs/kit';
import { getLevelDir } from 'src/viz/levelDef/levelPaths.server';

import { guardDev, validateName } from '../../levelEditorUtils.server';

/**
 * Server-Sent Events endpoint that watches the level's `geo/` directory for
 * changes and streams `geo-change` events to connected clients.
 *
 * Each event payload is `{ assetId: string, code: string }` — the caller
 * can immediately hot-reload the named asset without an additional round-trip.
 *
 * Dev only. Closes the stream immediately if the level has no `geo/` directory.
 */
export const GET: RequestHandler = ({ params }) => {
  guardDev();
  const name = validateName(params.name);

  const geoDir = join(getLevelDir(name), 'geo');
  const encoder = new TextEncoder();

  // Per-file debounce: many editors do atomic saves that trigger two rapid events.
  const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();

  let watcher: ReturnType<typeof watch> | null = null;

  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      if (!existsSync(geoDir)) {
        controller.close();
        return;
      }

      watcher = watch(geoDir, (_eventType, filename) => {
        if (!filename?.endsWith('.geo')) return;

        // Debounce per file: collapse rapid bursts into a single event.
        const existing = debounceTimers.get(filename);
        if (existing) clearTimeout(existing);
        debounceTimers.set(
          filename,
          setTimeout(() => {
            debounceTimers.delete(filename);

            let code: string;
            try {
              code = readFileSync(join(geoDir, filename), 'utf-8');
            } catch {
              return; // File deleted or temporarily unreadable — ignore.
            }

            const assetId = filename.slice(0, -4);
            const payload = JSON.stringify({ assetId, code });
            controller.enqueue(encoder.encode(`event: geo-change\ndata: ${payload}\n\n`));
          }, 50)
        );
      });
    },

    cancel() {
      watcher?.close();
      watcher = null;
      for (const t of debounceTimers.values()) clearTimeout(t);
      debounceTimers.clear();
    },
  });

  return new Response(body, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    },
  });
};
