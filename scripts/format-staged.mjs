#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';

const repoRoot = spawnSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).stdout.trim();
if (!repoRoot) {
  process.exit(0);
}
process.chdir(repoRoot);

const warn = message => process.stderr.write(`pre-commit: ${message}\n`);
const zsplit = out => out.split('\0').filter(Boolean);
const hash = file => createHash('sha1').update(readFileSync(file)).digest('hex');

function git(args) {
  const res = spawnSync('git', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (res.status !== 0) {
    process.stderr.write(`git ${args.join(' ')} failed:\n${res.stderr ?? ''}`);
    process.exit(1);
  }
  return res.stdout;
}

// a missing formatter is skipped with a warning; a formatter that runs and fails blocks the commit
function run(command, args, cwd) {
  const res = spawnSync(command, args, { cwd, encoding: 'utf8' });
  if (res.error?.code === 'ENOENT') {
    return false;
  }
  if (res.error || res.status !== 0) {
    process.stderr.write(
      `\npre-commit: \`${command} ${args.join(' ')}\`${cwd ? ` (in ${cwd})` : ''} failed\n`
    );
    process.stderr.write(`${res.error?.message ?? ''}${res.stdout ?? ''}${res.stderr ?? ''}\n`);
    process.exit(1);
  }
  return true;
}

const staged = zsplit(git(['diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z']));
const present = staged.filter(existsSync);
if (present.length === 0) {
  process.exit(0);
}

const hasWorkspaceTable = dir => /^\[workspace\]/m.test(readFileSync(join(dir, 'Cargo.toml'), 'utf8'));

// nearest enclosing Cargo.toml, climbing past member crates up to their workspace root
function rustRoot(file) {
  const candidates = [];
  for (let dir = dirname(join(repoRoot, file)); dir.startsWith(repoRoot); dir = dirname(dir)) {
    if (existsSync(join(dir, 'Cargo.toml'))) {
      candidates.push(dir);
    }
    if (dir === repoRoot) {
      break;
    }
  }
  let root = candidates[0];
  for (let i = 1; i < candidates.length && !hasWorkspaceTable(root); i++) {
    root = candidates[i];
  }
  return root;
}

const dirtyBefore = new Set(zsplit(git(['diff', '--name-only', '-z'])));
const before = new Map(present.map(file => [file, hash(file)]));

// oxfmt filters the list itself by extension and by its own ignore rules
const oxfmt = join(repoRoot, 'node_modules', 'oxfmt', 'bin', 'oxfmt');
if (existsSync(oxfmt)) {
  run(process.execPath, [oxfmt, '--no-error-on-unmatched-pattern', ...present]);
} else {
  warn('node_modules/oxfmt is missing — skipped JS/TS formatting (run `yarn install`)');
}

const rustRoots = [
  ...new Set(
    staged
      .filter(file => file.endsWith('.rs'))
      .map(rustRoot)
      .filter(Boolean)
  ),
];

// `cargo fmt` formats whole crates, so track the untracked .rs files it may touch too
const untrackedRust = new Map();
for (const root of rustRoots) {
  const others = zsplit(
    git(['ls-files', '--others', '--exclude-standard', '-z', '--', relative(repoRoot, root)])
  );
  for (const file of others) {
    if (file.endsWith('.rs') && !before.has(file)) {
      untrackedRust.set(file, hash(file));
    }
  }
}

for (const root of rustRoots) {
  if (!run('cargo', ['fmt', '--all'], root)) {
    warn(`cargo is not installed — skipped Rust formatting in ${relative(repoRoot, root)}`);
  }
}

const changed = present.filter(file => existsSync(file) && hash(file) !== before.get(file));
const restage = changed.filter(file => !dirtyBefore.has(file));
const partial = changed.filter(file => dirtyBefore.has(file));

if (restage.length > 0) {
  git(['add', '--', ...restage]);
  process.stdout.write(`pre-commit: formatted ${restage.length} staged file(s)\n`);
}

if (partial.length > 0) {
  warn(
    `WARNING — reformatted in the working tree but committed as-is (they have unstaged changes):\n  ${partial.join('\n  ')}`
  );
}

const collateral = [
  ...zsplit(git(['diff', '--name-only', '-z'])).filter(
    file => !dirtyBefore.has(file) && !changed.includes(file)
  ),
  ...[...untrackedRust].filter(([file, sha]) => existsSync(file) && hash(file) !== sha).map(([file]) => file),
];
if (collateral.length > 0) {
  process.stdout.write(
    `pre-commit: cargo fmt also reformatted ${collateral.length} file(s) outside this commit:\n  ${collateral.join('\n  ')}\n`
  );
}
