import { spawn } from 'node:child_process';
import { join } from 'node:path';

const moduleDir = join(process.cwd(), 'node_modules');
const useTsgo = process.env.CHECK_TSGO === '1';

function cliPath(...parts) {
  return join(moduleDir, ...parts);
}

function run(command, args, options = {}) {
  return new Promise(resolve => {
    const child = spawn(command, args, {
      env: { ...process.env, FORCE_COLOR: '0' },
      stdio: ['ignore', 'pipe', 'pipe'],
      ...options,
    });

    let stdout = '';
    let stderr = '';

    child.stdout.on('data', chunk => {
      stdout += chunk;
    });

    child.stderr.on('data', chunk => {
      stderr += chunk;
    });

    child.on('error', error => {
      resolve({ error, status: 1, stderr, stdout });
    });

    child.on('close', status => {
      resolve({ status: status ?? 1, stderr, stdout });
    });
  });
}

function writeOutput(output) {
  if (!output) {
    return;
  }
  process.stderr.write(output.endsWith('\n') ? output : `${output}\n`);
}

function fail(step, output, exitCode = 1) {
  process.stderr.write(`CHECK FAILED: ${step}\n`);
  writeOutput(output.trim());
  process.exit(exitCode || 1);
}

const syncResult = await run(process.execPath, [cliPath('@sveltejs', 'kit', 'svelte-kit.js'), 'sync']);
if (syncResult.error) {
  fail('sync', syncResult.error.message);
}
if (syncResult.status !== 0) {
  fail('sync', `${syncResult.stdout}${syncResult.stderr}`, syncResult.status);
}

const svelteMachineArgs = [
  cliPath('svelte-check', 'bin', 'svelte-check'),
  '--tsconfig',
  './tsconfig.json',
  '--output',
  'machine',
  '--fail-on-warnings',
  '--incremental',
];
if (useTsgo) {
  svelteMachineArgs.push('--tsgo');
}
const [svelteResult, formatResult, lintResult] = await Promise.all([
  run(process.execPath, svelteMachineArgs),
  run(process.execPath, [cliPath('oxfmt', 'bin', 'oxfmt'), '--list-different']),
  run(process.execPath, [cliPath('oxlint', 'bin', 'oxlint'), '--format', 'unix', '--deny-warnings']),
]);

if (svelteResult.error) {
  fail('svelte-check', svelteResult.error.message);
}
if (svelteResult.status !== 0) {
  process.stderr.write('CHECK FAILED: svelte-check\n');
  const svelteHumanResult = await run(process.execPath, [
    cliPath('svelte-check', 'bin', 'svelte-check'),
    '--tsconfig',
    './tsconfig.json',
    '--output',
    'human',
    '--fail-on-warnings',
    '--incremental',
    ...(useTsgo ? ['--tsgo'] : []),
  ]);
  writeOutput(`${svelteHumanResult.stdout}${svelteHumanResult.stderr}`.trim());
  process.exit(svelteResult.status || 1);
}

if (formatResult.error) {
  fail('format', formatResult.error.message);
}
let autoFormattedFiles = [];
if (formatResult.status !== 0) {
  autoFormattedFiles = formatResult.stdout.trim().split('\n').filter(Boolean);
  const fixResult = await run(process.execPath, [cliPath('oxfmt', 'bin', 'oxfmt'), ...autoFormattedFiles]);
  if (fixResult.error || fixResult.status !== 0) {
    fail('format', `oxfmt auto-fix failed:\n${fixResult.stdout}${fixResult.stderr}`, fixResult.status || 1);
  }
}

if (lintResult.error) {
  fail('lint', lintResult.error.message);
}
if (lintResult.status !== 0) {
  fail('lint', `${lintResult.stdout}${lintResult.stderr}`, lintResult.status);
}

if (autoFormattedFiles.length > 0) {
  process.stdout.write(
    `${autoFormattedFiles.length} file(s) auto-formatted by oxfmt:\n${autoFormattedFiles.join('\n')}\n`
  );
}
process.stdout.write('CHECK OK\n');
