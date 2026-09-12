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
 * `--target prod` sends the same requests through the prod backend's `/render/transient` proxy
 * (CLI token from `GEOTOY_CLI_TOKEN` or `.env.geotoy-cli`), so the deployed frontend + wasm are
 * what gets timed, and appends a trimmed record to `bench_history/prod/` for trend tracking.
 *
 *   bun scripts/geotoy-bench.ts run --out bench_results/<label> [--target dev|prod] [--ids 100-136,140]
 *       [--iterations 5] [--warmup 2] [--mode cold|warm] [--render] [--trace] [--tab <name>]
 *       [--timeout <s>] [--db <path>] [--service <url>] [--resume] [--history <dir>]
 *   bun scripts/geotoy-bench.ts summary <dir|record.json>
 *   bun scripts/geotoy-bench.ts compare <baseline> <candidate> [--metric eval|wall|...]
 *   bun scripts/geotoy-bench.ts history [<dir>] [--metric eval|wall|...]
 *   bun scripts/geotoy-bench.ts chart [<dir>] [--out <file.html>] [--metric eval|wall|...]
 */
import { Database } from 'bun:sqlite';
import { createHash } from 'crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'fs';
import { basename, join } from 'path';

const ROOT = join(import.meta.dir, '..');
const WASM_PATH = join(ROOT, 'src', 'viz', 'wasmComp', 'geoscript_repl_bg.wasm');
const DEV_SERVICE = 'http://localhost:5812/render_transient';
const PROD_SERVICE = 'https://3d.ameo.design/geotoy_api/render/transient';
const DEV_DB = join(ROOT, 'geoscript_backend', 'geoscript_backend.sqlite3');
/** Prod snapshot written by `scripts/sync-geo-compositions.ts`. */
const PROD_DB = join(ROOT, 'geoscript_backend.sqlite3');
const HISTORY_DIR = join(ROOT, 'bench_history', 'prod');
/** The backend proxy buffers the whole response under a 15 min client timeout. */
const PROD_PROXY_TIMEOUT_MS = 15 * 60_000;
const PROD_HOST = 'debian@ameo.dev';

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
  /** `git describe --always --dirty` of the frontend build (absent on builds before it was stamped). */
  build?: string;
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
  target?: 'dev' | 'prod';
  /** Frontend build as reported by the page; null until a composition succeeds. */
  build?: string | null;
  /** Prod container image tags at run time (`docker inspect` over ssh); a build id for pre-stamp deploys. */
  deployed?: Record<string, string> | null;
  config: Record<string, unknown>;
  /** The runner's local wasm; only meaningful for dev targets. */
  wasm: { sha256: string; bytes: number; mtime: string };
  assets?: Record<string, { sha256: string; bytes: number }>;
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

/** Include the external geometry engines; identical Rust Wasm does not imply identical runs. */
const assetInfo = () =>
  Object.fromEntries(
    [
      'src/viz/wasmComp/geoscript_repl_bg.wasm',
      'src/viz/wasm/cgal/index.wasm',
      'src/viz/wasm/cgal/index.js',
      'node_modules/manifold-3d/manifold.wasm',
      'node_modules/manifold-3d/manifold.js',
      'node_modules/meshoptimizer/meshopt_simplifier.js',
      'src/geoscript/meshopt.ts',
      'src/geoscript/manifold.ts',
      'src/geoscript/geodesics.ts',
      'src/geoscript/geodesicPathBuffers.ts',
    ]
      .filter(p => existsSync(join(ROOT, p)))
      .map(p => {
        const bytes = readFileSync(join(ROOT, p));
        return [p, { sha256: createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length }];
      })
  );

const cliToken = (): string => {
  if (process.env.GEOTOY_CLI_TOKEN) return process.env.GEOTOY_CLI_TOKEN;
  const p = join(ROOT, '.env.geotoy-cli');
  const m = existsSync(p) ? /^GEOTOY_CLI_TOKEN=(.+)$/m.exec(readFileSync(p, 'utf8')) : null;
  return (
    m?.[1].trim().replace(/^['"]|['"]$/g, '') ??
    die('--target prod needs GEOTOY_CLI_TOKEN (env or .env.geotoy-cli)')
  );
};

const deployedImages = (): Record<string, string> | null => {
  const proc = Bun.spawnSync(
    [
      'ssh',
      '-o',
      'BatchMode=yes',
      '-o',
      'ConnectTimeout=10',
      PROD_HOST,
      // one string: ssh re-joins argv unquoted on the remote side
      "docker inspect --format '{{.Name}} {{.Config.Image}}' dream geoscript-thumbnail-renderer",
    ],
    { timeout: 20_000 }
  );
  if (proc.exitCode !== 0) return null;
  return Object.fromEntries(
    proc.stdout
      .toString()
      .trim()
      .split('\n')
      .map(l => {
        const [name, image] = l.split(' ');
        return [name.replace(/^\//, ''), image];
      })
  );
};

const runCommand = async () => {
  const outDir = optVal('--out') ?? die('--out is required');
  const target = (optVal('--target') ?? 'dev') as 'dev' | 'prod';
  if (target !== 'dev' && target !== 'prod') die('--target must be dev or prod');
  const prod = target === 'prod';
  const dbPath = optVal('--db') ?? (prod ? PROD_DB : DEV_DB);
  const service = optVal('--service') ?? (prod ? PROD_SERVICE : DEV_SERVICE);
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (prod) headers['X-CLI-Token'] = cliToken();
  const historyDir = optVal('--history') ?? (prod ? HISTORY_DIR : null);
  const iterations = Number(optVal('--iterations') ?? 5);
  const warmup = Number(optVal('--warmup') ?? 2);
  const mode = (optVal('--mode') ?? 'cold') as 'cold' | 'warm';
  const render = has('--render');
  const trace = has('--trace');
  const tab = optVal('--tab');
  const timeoutMs = Math.min(
    Number(optVal('--timeout') ?? 600) * 1000,
    prod ? PROD_PROXY_TIMEOUT_MS - 30_000 : Infinity
  );
  const only = optVal('--ids') ? new Set(parseIds(optVal('--ids')!)) : null;
  const singleRunIds = optVal('--single-run-ids') ? parseIds(optVal('--single-run-ids')!) : [];
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
    target,
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
    singleRunIds,
  };
  const report: Report = {
    label: basename(outDir),
    createdAt: new Date().toISOString(),
    target,
    build: null,
    deployed: prod ? deployedImages() : undefined,
    config,
    wasm: wasmInfo(),
    assets: assetInfo(),
    git: gitInfo(),
    compositions: [],
    summary: buildSummary([], 0),
  };
  if (resume && existsSync(join(outDir, 'report.json'))) {
    const previous = JSON.parse(readFileSync(join(outDir, 'report.json'), 'utf8')) as Report;
    if (JSON.stringify(previous.assets) !== JSON.stringify(report.assets)) {
      die('Cannot resume: benchmark assets changed. Use a new output directory.');
    }
    report.build = previous.build ?? null;
    for (const key of ['target', 'iterations', 'warmup', 'mode', 'render', 'trace', 'db', 'singleRunIds']) {
      if (JSON.stringify(previous.config[key]) !== JSON.stringify(report.config[key]))
        die(`Cannot resume: ${key} changed.`);
    }
  }
  const provenance = prod
    ? `prod via ${service}; deployed ${report.deployed?.dream ?? '?'}`
    : `wasm ${report.wasm.sha256} @ ${report.git.head}${report.git.dirty ? ' dirty' : ''}`;
  console.log(
    `benching ${rows.length} compositions → ${outDir}  (${mode}, ${warmup} warmup + ${iterations} timed${trace ? ', traced' : ''}; ${provenance})`
  );

  const entryPath = (id: number) => join(outDir, `${id}.bench.json`);
  const flush = (totalElapsedMs: number) => {
    report.summary = buildSummary(report.compositions, totalElapsedMs);
    writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  };

  const t0 = Date.now();
  for (const row of rows) {
    if (!prod && JSON.stringify(assetInfo()) !== JSON.stringify(report.assets)) {
      flush(Date.now() - t0);
      die('Benchmark assets changed during the run; stopping to avoid mixed measurements.');
    }
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
      options: {
        dev: !prod,
        timeoutMs,
        bench: {
          iterations: singleRunIds.includes(row.id) ? 1 : iterations,
          warmup: singleRunIds.includes(row.id) ? 0 : warmup,
          mode,
          render,
        },
        trace,
      },
    };
    const started = Date.now();
    let entry: CompEntry;
    try {
      const res = await fetch(service, {
        method: 'POST',
        headers,
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
      if (report.build == null && bench.build) {
        report.build = bench.build;
        console.log(`  frontend build: ${bench.build}`);
      }
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
  if (historyDir) writeHistory(report, historyDir);
};

// --- history records -----------------------------------------------------------------------------

/** Trimmed report kept under version control: raw timed samples stay (for `compare`), boot,
 *  warmup, trace, and asset hashes go. */
const toHistoryRecord = (r: Report): Report => ({
  ...r,
  assets: undefined,
  compositions: r.compositions.map(c =>
    c.ok
      ? {
          ...c,
          trace: null,
          warmupRuns: [],
          runs: c.runs.map(({ constEvalCache: _, ...run }) => run as Sample),
        }
      : c
  ),
});

const buildOf = (r: Report): string =>
  r.build ??
  (r.target === 'prod'
    ? (r.deployed?.dream ?? 'unknown')
    : `wasm ${r.wasm.sha256} @ ${r.git.head}${r.git.dirty ? ' dirty' : ''}`);

const writeHistory = (r: Report, dir: string): string => {
  mkdirSync(dir, { recursive: true });
  const stamp = r.createdAt.replace(/[-:]/g, '').slice(0, 15);
  const build = (r.build ?? r.deployed?.dream?.split(':')[1] ?? 'unknown').replace(/[^\w.-]/g, '_');
  const p = join(dir, `${stamp}_${build}.json`);
  writeFileSync(p, JSON.stringify(toHistoryRecord(r)));
  console.log(`history record → ${p}`);
  return p;
};

const loadHistory = (dir: string): Report[] => {
  if (!existsSync(dir)) die(`${dir} not found`);
  return readdirSync(dir)
    .filter(f => f.endsWith('.json'))
    .map(f => loadReport(join(dir, f)))
    .sort((x, y) => x.createdAt.localeCompare(y.createdAt));
};

// --- summary / compare / history / chart -------------------------------------------------------

const loadReport = (p: string): Report => {
  const file = existsSync(p) && statSync(p).isDirectory() ? join(p, 'report.json') : p;
  if (!existsSync(file)) die(`${file} not found`);
  return JSON.parse(readFileSync(file, 'utf8'));
};

const summaryCommand = () => {
  const report = loadReport(args[1] ?? die('summary <dir|record.json>'));
  console.log(`${report.label}  ${report.createdAt}  ${report.target ?? 'dev'}  ${buildOf(report)}`);
  printTable(report.compositions);
  printSummary(report);
};

interface CompareRow {
  id: number;
  title: string;
  ma: number;
  mb: number;
  delta: number;
  p: number;
  sig: boolean;
}

/** Per-comp median delta B vs A over the compositions both runs completed; `sig` = p<0.05 and |Δ|>2%. */
const compareRows = (a: Report, b: Report, metric: string): CompareRow[] => {
  if (!(metric in METRICS)) die(`unknown metric ${metric}; one of ${Object.keys(METRICS).join(', ')}`);
  const byId = new Map(b.compositions.map(c => [c.id, c]));
  const rows: CompareRow[] = [];
  for (const ca of a.compositions) {
    const cb = byId.get(ca.id);
    if (!ca.ok || !cb || !cb.ok) continue;
    const xa = metricSamples(ca.runs, metric);
    const xb = metricSamples(cb.runs, metric);
    if (!xa.length || !xb.length) continue;
    const [ma, mb] = [median(xa), median(xb)];
    const delta = (mb - ma) / ma;
    const p = mannWhitneyP(xa, xb);
    rows.push({ id: ca.id, title: ca.title, ma, mb, delta, p, sig: p < 0.05 && Math.abs(delta) > 0.02 });
  }
  return rows.sort((x, y) => x.id - y.id);
};

const sumDelta = (rows: CompareRow[]) => {
  const sumA = rows.reduce((n, r) => n + r.ma, 0);
  const sumB = rows.reduce((n, r) => n + r.mb, 0);
  return { sumA, sumB, delta: (sumB - sumA) / sumA };
};
const pct = (x: number) => `${(x * 100).toFixed(1)}%`;

const printCompareRows = (rows: CompareRow[], onlySig = false) => {
  console.log(
    `${pad('id', 4)} ${pad('title', 30)} ${pad('A', 9, true)} ${pad('B', 9, true)} ${pad('delta', 8, true)} ${pad('p', 7, true)}`
  );
  for (const r of rows) {
    if (onlySig && !r.sig) continue;
    const flag = r.sig ? (r.delta < 0 ? ' ▼' : ' ▲') : '';
    console.log(
      `${pad(String(r.id), 4)} ${pad(r.title, 30)} ${pad(fmtMs(r.ma), 9, true)} ${pad(fmtMs(r.mb), 9, true)} ${pad(pct(r.delta), 8, true)} ${pad(Number.isFinite(r.p) ? r.p.toFixed(3) : '-', 7, true)}${flag}`
    );
  }
  const faster = rows.filter(r => r.sig && r.delta < 0).length;
  const slower = rows.filter(r => r.sig && r.delta > 0).length;
  const { sumA, sumB, delta } = sumDelta(rows);
  console.log(
    `\n${rows.length} compared: ${faster} faster, ${slower} slower (p<0.05, |Δ|>2%); sum of medians ${fmtMs(sumA)} → ${fmtMs(sumB)} (${pct(delta)})`
  );
};

const compareCommand = () => {
  const [a, b] = [
    loadReport(args[1] ?? die('compare <a> <b>')),
    loadReport(args[2] ?? die('compare <a> <b>')),
  ];
  const metric = optVal('--metric') ?? 'eval';
  console.log(
    `A: ${a.label}  ${buildOf(a)}\nB: ${b.label}  ${buildOf(b)}\nmetric: ${metric} (median of timed runs; p = two-sided Mann-Whitney)\n`
  );
  printCompareRows(compareRows(a, b, metric));
  if (!a.build && !b.build && a.wasm.sha256 === b.wasm.sha256)
    console.log('note: identical Geoscript Wasm builds; external engines may differ (see assets)');
};

const positional = (i: number) => (args[i] && !args[i].startsWith('--') ? args[i] : undefined);

const historyCommand = () => {
  const dir = positional(1) ?? HISTORY_DIR;
  const metric = optVal('--metric') ?? 'eval';
  const runs = loadHistory(dir);
  if (!runs.length) die(`no records in ${dir}`);
  console.log(
    `${runs.length} runs in ${dir}; Δ = sum of ${metric} medians vs the previous run over shared comps\n`
  );
  console.log(
    `${pad('date', 16)} ${pad('build', 24)} ${pad('ok', 6, true)} ${pad(`Σ${metric}`, 9, true)} ${pad('Σwall', 9, true)} ${pad('Δ', 8, true)}  sig`
  );
  runs.forEach((r, i) => {
    const prev = runs[i - 1];
    const rows = prev ? compareRows(prev, r, metric) : [];
    const delta = rows.length ? pct(sumDelta(rows).delta) : '';
    const sig = rows.length
      ? `${rows.filter(x => x.sig && x.delta < 0).length}▼ ${rows.filter(x => x.sig && x.delta > 0).length}▲`
      : '';
    console.log(
      `${pad(r.createdAt.slice(0, 16).replace('T', ' '), 16)} ${pad(buildOf(r), 24)} ${pad(`${r.summary.ok}/${r.summary.ok + r.summary.failed}`, 6, true)} ${pad(fmtMs(r.summary.sumOfMedians[metric] ?? NaN), 9, true)} ${pad(fmtMs(r.summary.sumOfMedians.wall ?? NaN), 9, true)} ${pad(delta, 8, true)}  ${sig}`
    );
  });
  if (runs.length >= 2) {
    const [a, b] = runs.slice(-2);
    console.log(`\nsignificant changes, ${buildOf(a)} → ${buildOf(b)}:\n`);
    printCompareRows(compareRows(a, b, metric), true);
  }
};

// --- chart ---------------------------------------------------------------------------------------

const esc = (s: string) =>
  s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

interface Series {
  name: string;
  points: (number | null)[];
  cls?: string;
}

/** Inline SVG line chart; null points break the line. */
const lineChart = (
  series: Series[],
  labels: string[],
  opts: { log?: boolean; height?: number } = {}
): string => {
  const W = 1000;
  const H = opts.height ?? 260;
  const [L, R, T, B] = [70, 20, 12, 44];
  const xs = labels.map((_, i) => L + (labels.length === 1 ? 0 : (i * (W - L - R)) / (labels.length - 1)));
  const vals = series.flatMap(s => s.points).filter((v): v is number => v !== null && v > 0);
  if (!vals.length) return '<p>no data</p>';
  const tf = opts.log ? Math.log10 : (v: number) => v;
  let [lo, hi] = [Math.min(...vals), Math.max(...vals)];
  if (!opts.log) lo = 0;
  if (hi === lo) hi = lo + 1;
  const y = (v: number) => T + (H - T - B) * (1 - (tf(v) - tf(lo)) / (tf(hi) - tf(lo)));
  const ticks = opts.log
    ? Array.from(
        { length: Math.ceil(Math.log10(hi)) - Math.floor(Math.log10(lo)) + 1 },
        (_, i) => 10 ** (Math.floor(Math.log10(lo)) + i)
      ).filter(t => t >= lo && t <= hi)
    : [0, 0.25, 0.5, 0.75, 1].map(f => lo + (hi - lo) * f);
  const grid = ticks
    .map(
      t =>
        `<line x1="${L}" x2="${W - R}" y1="${y(t).toFixed(1)}" y2="${y(t).toFixed(1)}" class="grid"/><text x="${L - 6}" y="${(y(t) + 4).toFixed(1)}" class="tick">${fmtMs(t)}</text>`
    )
    .join('');
  const xlabels = labels
    .map(
      (l, i) =>
        `<text x="${xs[i].toFixed(1)}" y="${H - B + 14}" class="tick x" transform="rotate(-25 ${xs[i].toFixed(1)} ${H - B + 14})">${esc(l)}</text>`
    )
    .join('');
  const paths = series
    .map(s => {
      let d = '';
      let pen = false;
      s.points.forEach((v, i) => {
        if (v === null || v <= 0) {
          pen = false;
          return;
        }
        d += `${pen ? 'L' : 'M'}${xs[i].toFixed(1)},${y(v).toFixed(1)} `;
        pen = true;
      });
      const dots = s.points
        .map((v, i) =>
          v === null || v <= 0 ? '' : `<circle cx="${xs[i].toFixed(1)}" cy="${y(v).toFixed(1)}" r="2.5"/>`
        )
        .join('');
      return `<g class="s ${s.cls ?? ''}" data-name="${esc(s.name)}"><title>${esc(s.name)}</title><path d="${d.trim()}"/>${dots}</g>`;
    })
    .join('');
  return `<svg viewBox="0 0 ${W} ${H}" width="${W}" height="${H}">${grid}${xlabels}${paths}</svg>`;
};

const chartCommand = () => {
  const dir = positional(1) ?? HISTORY_DIR;
  const metric = optVal('--metric') ?? 'eval';
  const out = optVal('--out') ?? join(dir, 'index.html');
  const runs = loadHistory(dir);
  if (!runs.length) die(`no records in ${dir}`);
  const labels = runs.map(r => `${r.createdAt.slice(0, 10)} ${buildOf(r)}`);
  const totals = [
    { name: `Σ ${metric} medians`, points: runs.map(r => r.summary.sumOfMedians[metric] ?? null), cls: 'a' },
    { name: 'Σ wall medians', points: runs.map(r => r.summary.sumOfMedians.wall ?? null), cls: 'b' },
  ];
  const ids = [...new Set(runs.flatMap(r => r.compositions.filter(c => c.ok).map(c => c.id)))].sort(
    (a, b) => a - b
  );
  const titleOf = new Map(runs.flatMap(r => r.compositions.map(c => [c.id, c.title] as const)));
  const perComp: Series[] = ids.map(id => ({
    name: `${id} ${titleOf.get(id)}`,
    points: runs.map(r => {
      const c = r.compositions.find(x => x.id === id);
      return c?.ok ? (c.summary[metric]?.median ?? null) : null;
    }),
  }));
  const [a, b] = runs.length >= 2 ? runs.slice(-2) : [null, null];
  const rows = a && b ? compareRows(a, b, metric) : [];
  const table = rows.length
    ? `<h2>latest vs previous <span class="dim">${esc(buildOf(a!))} → ${esc(buildOf(b!))}, ${metric} median, p = Mann-Whitney</span></h2>
<table><tr><th>id</th><th>title</th><th class="r">A</th><th class="r">B</th><th class="r">Δ</th><th class="r">p</th></tr>${rows
        .map(
          r =>
            `<tr class="${r.sig ? (r.delta < 0 ? 'faster' : 'slower') : ''}"><td>${r.id}</td><td>${esc(r.title)}</td><td class="r">${fmtMs(r.ma)}</td><td class="r">${fmtMs(r.mb)}</td><td class="r">${pct(r.delta)}</td><td class="r">${Number.isFinite(r.p) ? r.p.toFixed(3) : '-'}</td></tr>`
        )
        .join('')}</table>
<p>${rows.filter(r => r.sig && r.delta < 0).length} faster, ${rows.filter(r => r.sig && r.delta > 0).length} slower (p&lt;0.05, |Δ|&gt;2%); Σ ${fmtMs(sumDelta(rows).sumA)} → ${fmtMs(sumDelta(rows).sumB)} (${pct(sumDelta(rows).delta)})</p>`
    : '';
  const html = `<!doctype html><meta charset="utf-8"><title>geotoy bench history</title>
<style>
body{font:12px/1.45 ui-monospace,Menlo,Consolas,monospace;color:#111;background:#fff;margin:24px auto;max-width:1040px;padding:0 16px}
h1{font-size:15px;margin:0 0 4px}h2{font-size:13px;margin:28px 0 6px}.dim{color:#777;font-weight:normal}
svg{display:block;max-width:100%;height:auto}
.grid{stroke:#e5e5e5}.tick{font-size:10px;fill:#777;text-anchor:end}.tick.x{text-anchor:end}
.s path{fill:none;stroke:#bbb;stroke-width:1.2}.s circle{fill:#bbb}
.s.a path{stroke:#111;stroke-width:1.6}.s.a circle{fill:#111}.s.b path{stroke:#888;stroke-dasharray:4 3}.s.b circle{fill:#888}
#comps .s:hover path,#comps .s.hi path{stroke:#111;stroke-width:2}#comps .s:hover circle,#comps .s.hi circle{fill:#111}
#hover{min-height:1.4em;color:#111}
table{border-collapse:collapse;width:100%}td,th{padding:2px 8px;text-align:left;border-bottom:1px solid #eee}th{color:#777;font-weight:normal}.r{text-align:right}
tr.faster td{color:#0a7d2c}tr.slower td{color:#b3261e}
</style>
<h1>geotoy bench history <span class="dim">${esc(basename(dir))} · ${runs.length} runs · generated ${new Date().toISOString().slice(0, 16).replace('T', ' ')}</span></h1>
<h2>corpus totals <span class="dim">sum of per-composition medians; solid = ${metric}, dashed = wall</span></h2>
${lineChart(totals, labels)}
<h2>per composition <span class="dim">${metric} median, log scale; hover a line</span></h2>
<div id="hover">&nbsp;</div>
<div id="comps">${lineChart(perComp, labels, { log: true, height: 420 })}</div>
${table}
<script>
const hov=document.getElementById('hover');
document.querySelectorAll('#comps .s').forEach(g=>{g.addEventListener('mouseenter',()=>{hov.textContent=g.dataset.name;});g.addEventListener('mouseleave',()=>{hov.innerHTML='&nbsp;';});});
</script>
`;
  mkdirSync(dir, { recursive: true });
  writeFileSync(out, html);
  console.log(`${runs.length} runs, ${ids.length} compositions → ${out}`);
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
  case 'history':
    historyCommand();
    break;
  case 'chart':
    chartCommand();
    break;
  default:
    die('usage: geotoy-bench.ts run|summary|compare|history|chart … (see header comment)');
}
