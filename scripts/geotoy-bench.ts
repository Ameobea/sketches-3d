/**
 * Benchmarks compositions from a geoscript backend DB through the transient render path
 * (thumbnail_generator :5812 → dev frontend :4800). Each composition boots once in a fresh
 * headless Chrome, then re-runs in place for `--warmup` untimed + `--iterations` timed runs, so
 * the wasm instance, JIT tiers, and loaded async deps carry across samples. `cold` mode clears
 * every cross-run cache (const-eval, module exports, Clipper2 memos) before each run.
 *
 * Writes `<out>/<id>.bench.json` per composition, optional `<id>.trace.json.gz` DevTools traces
 * of the timed runs, and `<out>/report.json` (per-phase medians + corpus summary).
 *
 *   bun scripts/geotoy-bench.ts run --out bench_results/<label> [--ids 100-136,140] [--iterations 5]
 *       [--warmup 2] [--mode cold|warm] [--render] [--trace] [--tab <name>] [--timeout <s>]
 *       [--db <path>] [--service <url>] [--resume]
 *   bun scripts/geotoy-bench.ts summary <dir>
 *   bun scripts/geotoy-bench.ts compare <baseline-dir> <candidate-dir> [--metric eval|wall|...]
 */
import { Database } from 'bun:sqlite';
import { createHash } from 'crypto';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'fs';
import { join } from 'path';

const ROOT = join(import.meta.dir, '..');
const WASM_PATH = join(ROOT, 'src', 'viz', 'wasmComp', 'geoscript_repl_bg.wasm');

const args = process.argv.slice(2);
const cmd = args[0];
const optVal = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const has = (flag: string) => args.includes(flag);

function die(msg: string): never {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

// --- stats ---------------------------------------------------------------------------------

const quantile = (xs: number[], q: number): number => {
  const s = [...xs].sort((a, b) => a - b);
  if (!s.length) return NaN;
  const i = (s.length - 1) * q;
  const lo = Math.floor(i);
  const hi = Math.ceil(i);
  return s[lo] + (s[hi] - s[lo]) * (i - lo);
};
const median = (xs: number[]) => quantile(xs, 0.5);
const mad = (xs: number[]) => {
  const m = median(xs);
  return median(xs.map(x => Math.abs(x - m)));
};

interface MetricSummary {
  n: number;
  median: number;
  min: number;
  max: number;
  p90: number;
  mad: number;
}
const summarize = (xs: number[]): MetricSummary => ({
  n: xs.length,
  median: median(xs),
  min: Math.min(...xs),
  max: Math.max(...xs),
  p90: quantile(xs, 0.9),
  mad: mad(xs),
});

const erf = (x: number): number => {
  const t = 1 / (1 + 0.3275911 * Math.abs(x));
  const y =
    1 -
    ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) *
      t *
      Math.exp(-x * x);
  return x >= 0 ? y : -y;
};
const normCdf = (z: number) => 0.5 * (1 + erf(z / Math.SQRT2));

/** Two-sided Mann-Whitney U p-value (normal approximation, tie-averaged ranks). */
const mannWhitneyP = (a: number[], b: number[]): number => {
  const n1 = a.length;
  const n2 = b.length;
  if (n1 < 2 || n2 < 2) return NaN;
  const all = [...a.map(v => ({ v, g: 0 })), ...b.map(v => ({ v, g: 1 }))].sort((x, y) => x.v - y.v);
  const ranks = new Array<number>(all.length);
  for (let i = 0; i < all.length;) {
    let j = i;
    while (j + 1 < all.length && all[j + 1].v === all[i].v) j++;
    for (let k = i; k <= j; k++) ranks[k] = (i + j) / 2 + 1;
    i = j + 1;
  }
  let r1 = 0;
  all.forEach((x, k) => {
    if (x.g === 0) r1 += ranks[k];
  });
  const u1 = r1 - (n1 * (n1 + 1)) / 2;
  const u = Math.min(u1, n1 * n2 - u1);
  const sigma = Math.sqrt((n1 * n2 * (n1 + n2 + 1)) / 12);
  const z = (u - (n1 * n2) / 2) / sigma;
  return 2 * (1 - normCdf(Math.abs(z)));
};

// --- shapes ---------------------------------------------------------------------------------

interface Phases {
  setup: number;
  ambient: number;
  eval: number;
  evalWall: number;
  extract: number;
}
interface Sample {
  wall: number;
  phases: Phases;
  asyncDepRetries: number;
  constEvalCache: { entries: number; bytes: number; maxBytes: number };
  materials?: number;
  frame?: number;
}
/** The page's `onBenchReady` envelope. */
interface BenchEnvelope {
  ok: true;
  mode: 'cold' | 'warm';
  iterations: number;
  warmup: number;
  render: boolean;
  activeTab: string;
  activeTabKind: string | null;
  tabsRun: string[];
  bootMs: number;
  boot: { phases: Phases; asyncDepRetries: number; constEvalCache: Sample['constEvalCache'] };
  warmupRuns: Sample[];
  runs: Sample[];
  stats: { meshes: number; paths: number; lights: number; textures: number; vertices: number; faces: number };
  asyncDeps: string[];
  userAgent: string;
}
type CompEntry =
  | ({ id: number; title: string; ok: true; elapsedMs: number; trace: string | null } & BenchEnvelope & {
        summary: Record<string, MetricSummary>;
        retriesDuringTimed: number;
      })
  | { id: number; title: string; ok: false; error: string; elapsedMs: number };

interface Report {
  label: string;
  createdAt: string;
  config: Record<string, unknown>;
  wasm: { sha256: string; bytes: number; mtime: string };
  git: { head: string; dirty: number };
  compositions: CompEntry[];
  summary: {
    ok: number;
    failed: number;
    totalElapsedMs: number;
    evalMedianBuckets: Record<string, number[]>;
    sumOfMedians: Record<string, number>;
  };
}

/** Per-run metrics; `apply` is scene consume time (wall minus the runner's own phases). */
const METRICS: Record<string, (r: Sample) => number> = {
  eval: r => r.phases.eval,
  evalWall: r => r.phases.evalWall,
  setup: r => r.phases.setup,
  ambient: r => r.phases.ambient,
  extract: r => r.phases.extract,
  apply: r => r.wall - r.phases.setup - r.phases.ambient - r.phases.evalWall - r.phases.extract,
  wall: r => r.wall,
  materials: r => r.materials ?? NaN,
  frame: r => r.frame ?? NaN,
};
const metricSamples = (runs: Sample[], metric: string) =>
  runs.map(METRICS[metric]).filter(v => Number.isFinite(v));

const summarizeRuns = (runs: Sample[]): Record<string, MetricSummary> => {
  const out: Record<string, MetricSummary> = {};
  for (const m of Object.keys(METRICS)) {
    const xs = metricSamples(runs, m);
    if (xs.length) out[m] = summarize(xs);
  }
  return out;
};

const EVAL_BUCKETS: [string, number][] = [
  ['<50ms', 50],
  ['50-250ms', 250],
  ['250ms-1s', 1000],
  ['1-5s', 5000],
  ['5-20s', 20000],
  ['>20s', Infinity],
];

const buildSummary = (comps: CompEntry[], totalElapsedMs: number): Report['summary'] => {
  const oks = comps.filter((c): c is Extract<CompEntry, { ok: true }> => c.ok);
  const buckets: Record<string, number[]> = Object.fromEntries(EVAL_BUCKETS.map(([k]) => [k, []]));
  for (const c of oks) {
    const m = c.summary.eval.median;
    buckets[EVAL_BUCKETS.find(([, lim]) => m < lim)![0]].push(c.id);
  }
  const sumOfMedians: Record<string, number> = {};
  for (const m of Object.keys(METRICS)) {
    const vals = oks.map(c => c.summary[m]?.median).filter((v): v is number => v !== undefined);
    if (vals.length) sumOfMedians[m] = vals.reduce((a, b) => a + b, 0);
  }
  return {
    ok: oks.length,
    failed: comps.length - oks.length,
    totalElapsedMs,
    evalMedianBuckets: buckets,
    sumOfMedians,
  };
};

// --- printing -------------------------------------------------------------------------------

const fmtMs = (ms: number): string => {
  if (!Number.isFinite(ms)) return '-';
  if (ms >= 10_000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  return `${ms.toFixed(ms < 10 ? 1 : 0)}ms`;
};
const pad = (s: string, n: number, right = false) =>
  right ? s.padStart(n) : s.length > n ? s.slice(0, n - 1) + '…' : s.padEnd(n);

const printTable = (comps: CompEntry[]) => {
  console.log(
    `\n${pad('id', 4)} ${pad('title', 30)} ${pad('eval', 9, true)} ${pad('±mad', 7, true)} ${pad('min', 9, true)} ${pad('ambient', 8, true)} ${pad('extract', 8, true)} ${pad('apply', 8, true)} ${pad('wall', 9, true)}  tabs`
  );
  for (const c of [...comps].sort((a, b) => a.id - b.id)) {
    if (!c.ok) {
      console.log(`${pad(String(c.id), 4)} ${pad(c.title, 30)} ✗ ${c.error.split('\n')[0].slice(0, 90)}`);
      continue;
    }
    const s = c.summary;
    const tabs = c.tabsRun.length > 1 ? `[${c.tabsRun.join(', ')}]` : '';
    const retries = c.retriesDuringTimed ? ` !retries=${c.retriesDuringTimed}` : '';
    console.log(
      `${pad(String(c.id), 4)} ${pad(c.title, 30)} ${pad(fmtMs(s.eval.median), 9, true)} ${pad(fmtMs(s.eval.mad), 7, true)} ${pad(fmtMs(s.eval.min), 9, true)} ${pad(fmtMs(s.ambient.median), 8, true)} ${pad(fmtMs(s.extract.median), 8, true)} ${pad(fmtMs(s.apply.median), 8, true)} ${pad(fmtMs(s.wall.median), 9, true)}  ${tabs}${retries}`
    );
  }
};

const printSummary = (report: Report) => {
  const s = report.summary;
  console.log(`\n${s.ok} ok, ${s.failed} failed, ${fmtMs(s.totalElapsedMs)} total`);
  console.log('eval median distribution:');
  for (const [k, ids] of Object.entries(s.evalMedianBuckets)) {
    console.log(`  ${pad(k, 10)} ${String(ids.length).padStart(3)}  ${ids.join(' ')}`);
  }
  console.log(
    `sum of medians: ${Object.entries(s.sumOfMedians)
      .map(([k, v]) => `${k}=${fmtMs(v)}`)
      .join('  ')}`
  );
};

// --- run --------------------------------------------------------------------------------------

const parseIds = (spec: string): number[] =>
  spec.split(',').flatMap(part => {
    const m = /^(\d+)-(\d+)$/.exec(part.trim());
    if (m) {
      const [lo, hi] = [Number(m[1]), Number(m[2])];
      return Array.from({ length: hi - lo + 1 }, (_, i) => lo + i);
    }
    const n = Number(part);
    return Number.isFinite(n) ? [n] : die(`bad --ids segment: ${part}`);
  });

interface Row {
  id: number;
  title: string;
  tree: string;
  metadata: string;
}

const gitInfo = () => {
  const out = (c: string[]) => Bun.spawnSync(c, { cwd: ROOT }).stdout.toString().trim();
  return {
    head: out(['git', 'rev-parse', '--short', 'HEAD']),
    dirty: out(['git', 'status', '--porcelain']).split('\n').filter(Boolean).length,
  };
};
const wasmInfo = () => {
  const buf = readFileSync(WASM_PATH);
  return {
    sha256: createHash('sha256').update(buf).digest('hex').slice(0, 16),
    bytes: buf.length,
    mtime: statSync(WASM_PATH).mtime.toISOString(),
  };
};

const runCommand = async () => {
  const outDir = optVal('--out') ?? die('--out is required');
  const dbPath = optVal('--db') ?? join(ROOT, 'geoscript_backend', 'geoscript_backend.sqlite3');
  const service = optVal('--service') ?? 'http://localhost:5812/render_transient';
  const iterations = Number(optVal('--iterations') ?? 5);
  const warmup = Number(optVal('--warmup') ?? 2);
  const mode = (optVal('--mode') ?? 'cold') as 'cold' | 'warm';
  const render = has('--render');
  const trace = has('--trace');
  const tab = optVal('--tab');
  const timeoutMs = Number(optVal('--timeout') ?? 600) * 1000;
  const only = optVal('--ids') ? new Set(parseIds(optVal('--ids')!)) : null;
  const resume = has('--resume');
  if (!Number.isInteger(iterations) || iterations < 1) die('--iterations must be a positive integer');
  if (!Number.isInteger(warmup) || warmup < 0) die('--warmup must be a non-negative integer');
  if (mode !== 'cold' && mode !== 'warm') die('--mode must be cold or warm');
  mkdirSync(outDir, { recursive: true });

  const rows = (
    new Database(dbPath, { readonly: true })
      .query(
        `SELECT c.id, c.title, v.tree, v.metadata FROM compositions c
         JOIN composition_versions v ON v.id = (SELECT MAX(id) FROM composition_versions WHERE composition_id = c.id)
         ORDER BY c.id`
      )
      .all() as Row[]
  ).filter(r => !only || only.has(r.id));
  if (!rows.length) die('no compositions matched');

  const config = {
    iterations,
    warmup,
    mode,
    render,
    trace,
    tab: tab ?? null,
    timeoutMs,
    db: dbPath,
    service,
    ids: rows.map(r => r.id),
  };
  const report: Report = {
    label: outDir,
    createdAt: new Date().toISOString(),
    config,
    wasm: wasmInfo(),
    git: gitInfo(),
    compositions: [],
    summary: buildSummary([], 0),
  };
  console.log(
    `benching ${rows.length} compositions → ${outDir}  (${mode}, ${warmup} warmup + ${iterations} timed${trace ? ', traced' : ''}; wasm ${report.wasm.sha256} @ ${report.git.head}${report.git.dirty ? ' dirty' : ''})`
  );

  const entryPath = (id: number) => join(outDir, `${id}.bench.json`);
  const flush = (totalElapsedMs: number) => {
    report.summary = buildSummary(report.compositions, totalElapsedMs);
    writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  };

  const t0 = Date.now();
  for (const row of rows) {
    if (resume && existsSync(entryPath(row.id))) {
      report.compositions.push(JSON.parse(readFileSync(entryPath(row.id), 'utf8')));
      console.log(`↻ ${row.id} ${row.title} (resumed)`);
      continue;
    }
    const tree = JSON.parse(row.tree);
    const metadata = JSON.parse(row.metadata);
    if (tab) {
      const entry = tree.trees?.find((t: { id: string; name: string }) => t.name === tab || t.id === tab);
      if (entry) metadata.activeTreeId = entry.id;
      else console.warn(`  ! ${row.id}: no tab named ${tab}; using activeTreeId`);
    }
    const payload = {
      tree,
      metadata,
      options: { dev: true, timeoutMs, bench: { iterations, warmup, mode, render }, trace },
    };
    const started = Date.now();
    let entry: CompEntry;
    try {
      const res = await fetch(service, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: AbortSignal.timeout(timeoutMs + 60_000),
      });
      if (!res.ok) throw new Error(`${res.status}: ${(await res.text().catch(() => '')).slice(0, 2000)}`);
      // The service streams keep-alive whitespace and reports failures in-band.
      const { bench, traceGz, error } = (await res.json()) as {
        bench?: BenchEnvelope;
        traceGz: string | null;
        error?: string;
      };
      if (!bench) throw new Error(error ?? 'no bench payload in response');
      let traceFile: string | null = null;
      if (traceGz) {
        traceFile = `${row.id}.trace.json.gz`;
        writeFileSync(join(outDir, traceFile), Buffer.from(traceGz, 'base64'));
      }
      entry = {
        id: row.id,
        title: row.title,
        ok: true,
        elapsedMs: Date.now() - started,
        trace: traceFile,
        ...bench,
        summary: summarizeRuns(bench.runs),
        retriesDuringTimed: bench.runs.reduce((n, r) => n + r.asyncDepRetries, 0),
      };
      const s = entry.summary;
      const tabs = entry.tabsRun.length > 1 ? `  [${entry.tabsRun.join(', ')}]` : '';
      console.log(
        `✓ ${row.id} ${row.title}  eval ${fmtMs(s.eval.median)} ±${fmtMs(s.eval.mad)} (min ${fmtMs(s.eval.min)})  wall ${fmtMs(s.wall.median)}  boot ${fmtMs(entry.bootMs)}${tabs}${entry.retriesDuringTimed ? '  !retries' : ''}  ${fmtMs(entry.elapsedMs)}`
      );
    } catch (err) {
      entry = {
        id: row.id,
        title: row.title,
        ok: false,
        error: err instanceof Error ? err.message : String(err),
        elapsedMs: Date.now() - started,
      };
      console.log(`✗ ${row.id} ${row.title}: ${entry.error.split('\n')[0].slice(0, 200)}`);
    }
    writeFileSync(entryPath(row.id), JSON.stringify(entry, null, 2));
    report.compositions.push(entry);
    flush(Date.now() - t0);
  }
  flush(Date.now() - t0);
  printTable(report.compositions);
  printSummary(report);
};

// --- summary / compare ---------------------------------------------------------------------------

const loadReport = (dir: string): Report => {
  const p = join(dir, 'report.json');
  if (!existsSync(p)) die(`${p} not found`);
  return JSON.parse(readFileSync(p, 'utf8'));
};

const summaryCommand = () => {
  const report = loadReport(args[1] ?? die('summary <dir>'));
  console.log(
    `${report.label}  ${report.createdAt}  wasm ${report.wasm.sha256} @ ${report.git.head}${report.git.dirty ? ' dirty' : ''}`
  );
  printTable(report.compositions);
  printSummary(report);
};

const compareCommand = () => {
  const [a, b] = [
    loadReport(args[1] ?? die('compare <a> <b>')),
    loadReport(args[2] ?? die('compare <a> <b>')),
  ];
  const metric = optVal('--metric') ?? 'eval';
  if (!(metric in METRICS)) die(`unknown metric ${metric}; one of ${Object.keys(METRICS).join(', ')}`);
  console.log(
    `A: ${a.label}  wasm ${a.wasm.sha256} @ ${a.git.head}\nB: ${b.label}  wasm ${b.wasm.sha256} @ ${b.git.head}\nmetric: ${metric} (median of timed runs; p = two-sided Mann-Whitney)\n`
  );
  const byId = new Map(b.compositions.map(c => [c.id, c]));
  const rows: { id: number; title: string; ma: number; mb: number; delta: number; p: number }[] = [];
  let sumA = 0;
  let sumB = 0;
  for (const ca of a.compositions) {
    const cb = byId.get(ca.id);
    if (!ca.ok || !cb || !cb.ok) continue;
    const xa = metricSamples(ca.runs, metric);
    const xb = metricSamples(cb.runs, metric);
    if (!xa.length || !xb.length) continue;
    const [ma, mb] = [median(xa), median(xb)];
    sumA += ma;
    sumB += mb;
    rows.push({ id: ca.id, title: ca.title, ma, mb, delta: (mb - ma) / ma, p: mannWhitneyP(xa, xb) });
  }
  console.log(
    `${pad('id', 4)} ${pad('title', 30)} ${pad('A', 9, true)} ${pad('B', 9, true)} ${pad('delta', 8, true)} ${pad('p', 7, true)}`
  );
  let better = 0;
  let worse = 0;
  for (const r of rows.sort((x, y) => x.id - y.id)) {
    const sig = r.p < 0.05 && Math.abs(r.delta) > 0.02;
    if (sig && r.delta < 0) {
      better++;
    } else if (sig) {
      worse++;
    }
    const flag = sig ? (r.delta < 0 ? ' ▼' : ' ▲') : '';
    console.log(
      `${pad(String(r.id), 4)} ${pad(r.title, 30)} ${pad(fmtMs(r.ma), 9, true)} ${pad(fmtMs(r.mb), 9, true)} ${pad(`${(r.delta * 100).toFixed(1)}%`, 8, true)} ${pad(Number.isFinite(r.p) ? r.p.toFixed(3) : '-', 7, true)}${flag}`
    );
  }
  console.log(
    `\n${rows.length} compared: ${better} faster, ${worse} slower (p<0.05, |Δ|>2%); sum of medians ${fmtMs(sumA)} → ${fmtMs(sumB)} (${(((sumB - sumA) / sumA) * 100).toFixed(1)}%)`
  );
  if (a.wasm.sha256 === b.wasm.sha256) console.log('note: identical wasm builds');
};

switch (cmd) {
  case 'run':
    await runCommand();
    break;
  case 'summary':
    summaryCommand();
    break;
  case 'compare':
    compareCommand();
    break;
  default:
    die('usage: geotoy-bench.ts run|summary|compare … (see header comment)');
}
