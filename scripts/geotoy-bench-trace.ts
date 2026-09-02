/**
 * Aggregates the DevTools traces captured by `geotoy-bench.ts run --trace`: CPU-profile self
 * and inclusive time per function, bucketed by where the code lives (geoscript wasm, manifold,
 * three, GC, …), per composition and corpus-wide. Writes `<dir>/trace-analysis.json` and prints
 * the top functions.
 *
 *   bun scripts/geotoy-bench-trace.ts <bench-dir> [--top 40] [--ids 100-136] [--thread worker|main|all]
 *       [--inclusive] [--per-comp 8]
 */
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { gunzipSync } from 'zlib';
import { join } from 'path';

const args = process.argv.slice(2);
const dir = args[0] ?? die('usage: geotoy-bench-trace.ts <bench-dir> …');
const optVal = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const top = Number(optVal('--top') ?? 40);
const perComp = Number(optVal('--per-comp') ?? 8);
const thread = optVal('--thread') ?? 'worker';
const showInclusive = args.includes('--inclusive');
const only = optVal('--ids')
  ? new Set(
      optVal('--ids')!
        .split(',')
        .flatMap(p => {
          const m = /^(\d+)-(\d+)$/.exec(p);
          return m ? Array.from({ length: +m[2] - +m[1] + 1 }, (_, i) => +m[1] + i) : [Number(p)];
        })
    )
  : null;

function die(msg: string): never {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

interface CallFrame {
  functionName: string;
  url?: string;
  codeType?: string;
  lineNumber?: number;
}
interface TraceEvent {
  ph: string;
  name: string;
  pid: number;
  tid: number;
  id?: string;
  args?: { name?: string; data?: any };
}

/** Rust crate hashes (`geoscript[e94a…]::foo`) and closure ids add nothing to aggregation. */
const cleanName = (n: string) =>
  n.replace(/\[[0-9a-f]{16}\]/g, '').replace(/::\{closure#\d+\}/g, '::{closure}');

const bucketOf = (cf: CallFrame): string => {
  const url = cf.url ?? '';
  const fn = cf.functionName;
  if (fn === '(garbage collector)') return 'gc';
  if (fn === '(program)' || fn === '(root)') return 'program';
  if (fn === '(idle)') return 'idle';
  if (cf.codeType === 'wasm' || url.includes('.wasm')) {
    if (url.includes('geoscript_repl')) return 'wasm:geoscript';
    if (url.includes('manifold')) return 'wasm:manifold';
    if (url.includes('uv-unwrap') || url.includes('uv_unwrap')) return 'wasm:uv-unwrap';
    if (url.includes('uv_solvers')) return 'wasm:uv-solvers';
    if (url.includes('cgal')) return 'wasm:cgal';
    if (url.includes('clipper')) return 'wasm:clipper2';
    if (url.includes('geodesics')) return 'wasm:geodesics';
    if (fn.startsWith('js-to-wasm') || fn.startsWith('wasm-to-js')) return 'wasm:bridge';
    // Emscripten modules load from hashed `wasm://` URLs; fall back to symbol prefixes.
    if (fn.startsWith('CGAL::') || fn.includes('cgal')) return 'wasm:cgal';
    if (fn.startsWith('Clipper2Lib::')) return 'wasm:clipper2';
    if (fn.startsWith('manifold::')) return 'wasm:manifold';
    if (fn.startsWith('geoscript::') || fn.startsWith('<geoscript::')) return 'wasm:geoscript';
    return 'wasm:other';
  }
  if (
    url.includes('geoscript_repl.js') ||
    fn.startsWith('imports.wbg') ||
    /passStringToWasm|getStringFromWasm|getArrayF32|getArrayU32/.test(fn)
  )
    return 'js:wasm-glue';
  if (url.includes('/three') || url.includes('three.js')) return 'js:three';
  if (url.includes('comlink')) return 'js:comlink';
  if (url.includes('manifold')) return 'js:manifold-glue';
  if (url.includes('/src/')) return 'js:app';
  if (url.includes('node_modules')) return 'js:deps';
  return 'js:other';
};

interface FnAgg {
  self: number;
  inclusive: number;
  bucket: string;
}
interface Analysis {
  totalMs: number;
  buckets: Record<string, number>;
  fns: Record<string, FnAgg>;
}

const analyzeTrace = (path: string): Analysis => {
  const events: TraceEvent[] = JSON.parse(gunzipSync(readFileSync(path)).toString()).traceEvents;
  const threadNames = new Map<string, string>();
  for (const e of events) {
    if (e.ph === 'M' && e.name === 'thread_name') threadNames.set(`${e.pid}:${e.tid}`, e.args!.name!);
  }
  const wantThread = (key: string) => {
    const name = threadNames.get(key) ?? '';
    if (thread === 'all') return true;
    if (thread === 'main') return name === 'CrRendererMain';
    return name === 'DedicatedWorker thread';
  };

  // `Profile` is emitted on the sampled thread; its chunks arrive on V8's profiler thread and
  // link back through `id`. Nodes accumulate across a profile's chunks.
  const profileThread = new Map<string, string>();
  for (const e of events) {
    if (e.name === 'Profile') profileThread.set(`${e.pid}:${e.id}`, `${e.pid}:${e.tid}`);
  }
  const profiles = new Map<
    string,
    { nodes: Map<number, { cf: CallFrame; parent?: number }>; samples: number[]; deltas: number[] }
  >();
  for (const e of events) {
    if (e.name !== 'ProfileChunk') continue;
    const key = `${e.pid}:${e.id}`;
    const sampledThread = profileThread.get(key);
    if (!sampledThread || !wantThread(sampledThread)) continue;
    let p = profiles.get(key);
    if (!p) {
      p = { nodes: new Map(), samples: [], deltas: [] };
      profiles.set(key, p);
    }
    const d = e.args!.data;
    for (const n of d.cpuProfile?.nodes ?? []) p.nodes.set(n.id, { cf: n.callFrame, parent: n.parent });
    p.samples.push(...(d.cpuProfile?.samples ?? []));
    p.deltas.push(...(d.timeDeltas ?? []));
  }

  // Emscripten modules load from `wasm://wasm/<hash>` URLs and may lack name sections, so an
  // unnamed frame takes the bucket its URL's named frames voted for.
  const urlVotes = new Map<string, Record<string, number>>();
  for (const p of profiles.values()) {
    for (const { cf } of p.nodes.values()) {
      const b = bucketOf(cf);
      if (!cf.url || !b.startsWith('wasm:') || b === 'wasm:other' || b === 'wasm:bridge') continue;
      let votes = urlVotes.get(cf.url);
      if (!votes) {
        votes = {};
        urlVotes.set(cf.url, votes);
      }
      votes[b] = (votes[b] ?? 0) + 1;
    }
  }
  const GLUE_MODULES: [string, string][] = [
    ['clipper2', 'wasm:clipper2'],
    ['cgal', 'wasm:cgal'],
    ['manifold', 'wasm:manifold'],
    ['uvUnwrap', 'wasm:uv-unwrap'],
    ['uvSolvers', 'wasm:uv-solvers'],
    ['geodesics', 'wasm:geodesics'],
  ];
  /** A module with no names at all (Clipper2) is attributed by the JS glue that called into it. */
  const bucketForNode = (nodes: Map<number, { cf: CallFrame; parent?: number }>, id: number): string => {
    const cf = nodes.get(id)!.cf;
    const b = bucketOf(cf);
    if (b !== 'wasm:other' || !cf.url) return b;
    const votes = Object.entries(urlVotes.get(cf.url) ?? {}).sort((x, y) => y[1] - x[1]);
    if (votes[0]) return votes[0][0];
    for (let cur = nodes.get(id)?.parent; cur !== undefined; cur = nodes.get(cur)?.parent) {
      const url = nodes.get(cur)?.cf.url ?? '';
      if (!url || url.includes('.wasm')) continue;
      const hit = GLUE_MODULES.find(([k]) => url.includes(k));
      if (hit) return hit[1];
    }
    return b;
  };

  const fns: Record<string, FnAgg> = {};
  const buckets: Record<string, number> = {};
  let totalMs = 0;
  for (const p of profiles.values()) {
    const nameCache = new Map<number, string>();
    const nameOf = (id: number) => {
      let n = nameCache.get(id);
      if (n === undefined) {
        n = cleanName(p.nodes.get(id)?.cf.functionName || '(anonymous)');
        nameCache.set(id, n);
      }
      return n;
    };
    for (let i = 0; i < p.samples.length; i++) {
      // A sample lasts until the next one; the final sample gets its own delta.
      const ms = (i + 1 < p.deltas.length ? p.deltas[i + 1] : p.deltas[i]) / 1000;
      const node = p.nodes.get(p.samples[i]);
      if (!node) continue;
      const bucket = bucketForNode(p.nodes, p.samples[i]);
      if (bucket === 'idle') continue;
      totalMs += ms;
      buckets[bucket] = (buckets[bucket] ?? 0) + ms;
      const name = nameOf(p.samples[i]);
      const agg = (fns[name] ??= { self: 0, inclusive: 0, bucket });
      agg.self += ms;
      const seen = new Set<string>();
      for (let id: number | undefined = p.samples[i]; id !== undefined; id = p.nodes.get(id)?.parent) {
        const n = nameOf(id);
        if (seen.has(n)) continue;
        seen.add(n);
        (fns[n] ??= { self: 0, inclusive: 0, bucket: bucketForNode(p.nodes, id) }).inclusive += ms;
      }
    }
  }
  return { totalMs, buckets, fns };
};

const fmt = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms.toFixed(1)}ms`);
const pct = (x: number, total: number) => `${((100 * x) / total).toFixed(1)}%`;
const short = (s: string, n = 100) => (s.length > n ? s.slice(0, n - 1) + '…' : s);

const printTop = (fns: Record<string, FnAgg>, total: number, n: number, key: 'self' | 'inclusive') => {
  const rows = Object.entries(fns)
    .filter(([name]) => !['(root)', '(program)'].includes(name))
    .sort((a, b) => b[1][key] - a[1][key])
    .slice(0, n);
  for (const [name, a] of rows) {
    console.log(
      `  ${fmt(a[key]).padStart(9)} ${pct(a[key], total).padStart(6)}  ${a.bucket.padEnd(16)} ${short(name)}`
    );
  }
};

const printBuckets = (buckets: Record<string, number>, total: number) => {
  for (const [b, ms] of Object.entries(buckets).sort((a, b) => b[1] - a[1])) {
    console.log(`  ${fmt(ms).padStart(9)} ${pct(ms, total).padStart(6)}  ${b}`);
  }
};

const files = readdirSync(dir)
  .filter(f => f.endsWith('.trace.json.gz'))
  .map(f => ({ id: Number(f.split('.')[0]), path: join(dir, f) }))
  .filter(f => !only || only.has(f.id))
  .sort((a, b) => a.id - b.id);
if (!files.length) die(`no *.trace.json.gz in ${dir}`);

const titles = new Map<number, string>();
const reportPath = join(dir, 'report.json');
if (existsSync(reportPath)) {
  for (const c of JSON.parse(readFileSync(reportPath, 'utf8')).compositions) titles.set(c.id, c.title);
}

const corpus: Analysis = { totalMs: 0, buckets: {}, fns: {} };
const perCompOut: Record<
  number,
  { title: string; totalMs: number; buckets: Record<string, number>; topSelf: [string, FnAgg][] }
> = {};
console.log(`thread: ${thread}; ${files.length} traces\n`);
for (const f of files) {
  const a = analyzeTrace(f.path);
  corpus.totalMs += a.totalMs;
  for (const [b, ms] of Object.entries(a.buckets)) corpus.buckets[b] = (corpus.buckets[b] ?? 0) + ms;
  for (const [name, agg] of Object.entries(a.fns)) {
    const c = (corpus.fns[name] ??= { self: 0, inclusive: 0, bucket: agg.bucket });
    c.self += agg.self;
    c.inclusive += agg.inclusive;
  }
  const title = titles.get(f.id) ?? '';
  const topSelf = Object.entries(a.fns)
    .filter(([name]) => !['(root)', '(program)'].includes(name))
    .sort((x, y) => y[1].self - x[1].self)
    .slice(0, perComp);
  perCompOut[f.id] = { title, totalMs: a.totalMs, buckets: a.buckets, topSelf };
  if (perComp > 0) {
    console.log(`── ${f.id} ${title}  (${fmt(a.totalMs)} sampled)`);
    printBuckets(a.buckets, a.totalMs);
    console.log('  top self:');
    for (const [name, agg] of topSelf) {
      console.log(
        `    ${fmt(agg.self).padStart(9)} ${pct(agg.self, a.totalMs).padStart(6)}  ${short(name, 90)}`
      );
    }
    console.log();
  }
}

console.log(`══ corpus (${files.length} traces, ${fmt(corpus.totalMs)} sampled) — buckets`);
printBuckets(corpus.buckets, corpus.totalMs);
console.log(`\n══ corpus top ${top} by self time`);
printTop(corpus.fns, corpus.totalMs, top, 'self');
if (showInclusive) {
  console.log(`\n══ corpus top ${top} by inclusive time`);
  printTop(corpus.fns, corpus.totalMs, top, 'inclusive');
}

const topFns = Object.fromEntries(
  Object.entries(corpus.fns)
    .sort((a, b) => b[1].self - a[1].self)
    .slice(0, 400)
);
writeFileSync(
  join(dir, `trace-analysis${thread === 'worker' ? '' : `-${thread}`}.json`),
  JSON.stringify(
    { thread, totalMs: corpus.totalMs, buckets: corpus.buckets, fns: topFns, compositions: perCompOut },
    null,
    2
  )
);
