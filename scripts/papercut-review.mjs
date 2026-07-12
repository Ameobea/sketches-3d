import { readdirSync, statSync, readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import os from 'node:os';
import { appendEntry } from './papercut-lib.mjs';

const MODEL = process.env.PAPERCUT_REVIEW_MODEL || 'haiku';
const CAP = 400_000; // ~100k tokens of rendered transcript
const clip = (s, n) => (s.length > n ? s.slice(0, n) + '…' : s);

function latestSession() {
  const dir = join(os.homedir(), '.claude', 'projects', process.cwd().replace(/[/.]/g, '-'));
  if (!existsSync(dir)) return null;
  const files = readdirSync(dir)
    .filter(f => f.endsWith('.jsonl'))
    .map(f => join(dir, f))
    .map(p => ({ p, m: statSync(p).mtimeMs }))
    .sort((a, b) => b.m - a.m);
  return files[0]?.p ?? null;
}

function render(file) {
  const out = [];
  for (const line of readFileSync(file, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    let e;
    try {
      e = JSON.parse(line);
    } catch {
      continue;
    }
    const msg = e.message;
    if (!msg?.content) continue;
    const blocks = Array.isArray(msg.content) ? msg.content : [{ type: 'text', text: msg.content }];
    const parts = [];
    for (const b of blocks) {
      if (b.type === 'text' && b.text) parts.push(clip(b.text, 1200));
      else if (b.type === 'tool_use')
        parts.push(`[tool ${b.name}] ${clip(JSON.stringify(b.input ?? {}), 300)}`);
      else if (b.type === 'tool_result') {
        const c = Array.isArray(b.content) ? b.content.map(x => x.text ?? '').join(' ') : b.content;
        parts.push(`[result] ${clip(typeof c === 'string' ? c : JSON.stringify(c), 700)}`);
      }
    }
    if (parts.length) out.push(`### ${msg.role || e.type}\n${parts.join('\n')}`);
  }
  const text = out.join('\n\n');
  return text.length > CAP ? text.slice(text.length - CAP) : text;
}

const PROMPT = `You are mining a Claude Code session transcript for "papercuts": small, non-blocking frictions the agent hit while working — a tool call that missed and had to be retried, an undocumented or surprising setup step, a flaky command, a stale cache, a misleading error, a non-obvious gotcha, a wrong path/flag/env var that had to be corrected. Ignore normal successful work, bugs in the code being written, and the user's feature requests.

For each distinct papercut, output ONE line: "PAPERCUT: " followed by one or two sentences as "what the agent was doing → what got in the way" (a guess at the cause/fix is a bonus). Deduplicate. Output ONLY PAPERCUT: lines and nothing else — no preamble, no numbering. If there are none, output nothing.`;

const session = latestSession();
if (!session) {
  console.error('no session transcript found for this project');
  process.exit(1);
}

const res = spawnSync('claude', ['-p', '--model', MODEL], {
  input: `${PROMPT}\n\n=== TRANSCRIPT ===\n\n${render(session)}`,
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
});
if (res.error) {
  console.error(`failed to run claude: ${res.error.message}`);
  process.exit(1);
}
if (res.status !== 0) {
  console.error(res.stderr?.trim() || `claude exited ${res.status}`);
  process.exit(1);
}

const found = res.stdout
  .split('\n')
  .map(l => l.trim())
  .filter(l => l.startsWith('PAPERCUT:'))
  .map(l => l.slice('PAPERCUT:'.length).trim())
  .filter(Boolean);

if (!found.length) {
  console.log('no papercuts found in the latest session.');
  process.exit(0);
}
for (const m of found) appendEntry(`${MODEL} (review)`, m);
console.log(`appended ${found.length} papercut(s) → PAPERCUTS.md`);
