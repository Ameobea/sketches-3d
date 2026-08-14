// Rejects raw C0 control bytes in source: a literal one makes git treat the whole file as
// binary. Not an oxlint rule because oxlint only sees JS/TS, and this lands just as easily in
// `.svelte`, `.rs` or `.sql`.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { extname } from 'node:path';

const TEXT_EXTS = new Set([
  '.ts',
  '.tsx',
  '.js',
  '.mjs',
  '.cjs',
  '.svelte',
  '.rs',
  '.sql',
  '.css',
  '.html',
  '.json',
  '.geo',
  '.frag',
  '.vert',
  '.glsl',
  '.wgsl',
  '.toml',
  '.yml',
  '.yaml',
  '.sh',
  '.md',
]);

/** Tab, newline and carriage return are the only C0 bytes that legitimately appear in source. */
const FORBIDDEN = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/;

const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
  encoding: 'buffer',
  maxBuffer: 64 * 1024 * 1024,
})
  .toString('utf8')
  .split('\0')
  .filter(f => f && TEXT_EXTS.has(extname(f)));

const problems = [];
for (const file of files) {
  let text;
  try {
    text = readFileSync(file, 'latin1');
  } catch {
    continue;
  }
  // Whole-file reject first; line/column is only worth computing for a file that actually hits.
  if (!FORBIDDEN.test(text)) continue;
  text.split('\n').forEach((line, ix) => {
    const col = line.search(FORBIDDEN);
    if (col >= 0) {
      const code = line.charCodeAt(col).toString(16).padStart(4, '0').toUpperCase();
      problems.push(`${file}:${ix + 1}:${col + 1}: raw control byte U+${code}`);
    }
  });
}

if (problems.length > 0) {
  process.stderr.write(`${problems.slice(0, 50).join('\n')}\n`);
  process.stderr.write(
    `\n${problems.length} raw control byte(s) in source. Use an escape (e.g. '\\u0000') instead —\n` +
      'a literal control byte makes git treat the file as binary, hiding it from diff and blame.\n'
  );
  process.exit(1);
}
