import { appendFileSync, existsSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

export const FILE = join(dirname(dirname(fileURLToPath(import.meta.url))), 'PAPERCUTS.md');

export const HEADER = `# Papercuts

Small frictions hit while working in this repo — a tool call that missed and had
to be retried, an undocumented setup step, a flaky command, a stale cache, a
misleading error, a non-obvious gotcha. None are blocking on their own; logged
together they show where the repo needs sanding down. Distinct from git history
(what changed) and from real tracked bugs.

Append in the moment with \`yarn papercut -m <model> "what you were doing → what got in the way"\`,
or mine the whole current session at once with \`yarn papercut:review\` (the \`/papercut\` slash command).
`;

export function author() {
  try {
    return (
      execFileSync('git', ['config', 'user.name'], { encoding: 'utf8' }).trim() || os.userInfo().username
    );
  } catch {
    return os.userInfo().username;
  }
}

export function appendEntry(model, message, who = author()) {
  if (!existsSync(FILE)) writeFileSync(FILE, HEADER);
  appendFileSync(FILE, `\n## ${new Date().toISOString()} — ${model} — ${who}\n\n${message}\n`);
}
