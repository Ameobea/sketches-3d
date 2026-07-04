import * as fs from 'node:fs';
import * as path from 'node:path';
import { randomUUID } from 'node:crypto';

const PROD_BACKEND = 'https://3d.ameo.design/geotoy_api/render/transient';
const DEV_BACKEND = 'http://localhost:5810/render/transient';

const ROOT_NODE_NAME = '_root';

interface Transform3 {
  pos: [number, number, number];
  rot: [number, number, number];
  scale: [number, number, number];
}

interface Instance extends Transform3 {
  id: string;
}

interface NodeDef {
  id: string;
  name: string;
  source: string;
  /** Per-node placements; length >= 1. A single identity instance = one un-transformed copy. */
  instances: Instance[];
  children: string[];
  disabled?: boolean;
}

interface TreeDef {
  version: 1;
  rootId: string;
  globalsSource: string;
  nodes: Record<string, NodeDef>;
}

interface ViewDef {
  cameraPosition: [number, number, number];
  target: [number, number, number];
  fov?: number;
  zoom?: number;
}

type MaterialOverride = 'normal' | 'wireframe' | 'wireframe-xray';

type MeshOutputFormat = 'summary' | 'glb' | 'gltf' | 'obj' | 'json';

interface EvalRequest {
  expr?: string;
  samples?: number;
  meshes?: MeshOutputFormat;
}

interface RenderOptions {
  format?: 'png' | 'avif' | 'jpeg';
  width?: number;
  height?: number;
  quality?: number;
  dev?: boolean;
  timeoutMs?: number;
  /** Override all mesh materials with a debug material (mirrors the app's n / w / shift+w keybinds). */
  materialOverride?: MaterialOverride;
  /** Present for `geotoy eval`: serialize run outputs to JSON instead of rendering an image. */
  eval?: EvalRequest;
}

interface TransientPayload {
  tree: TreeDef;
  metadata: {
    view?: ViewDef;
    materials?: unknown;
    preludeEjected?: boolean;
    environment?: unknown;
  };
  options?: RenderOptions;
}

type Opts = Record<string, string | boolean>;

const identityTransform = (): Transform3 => ({ pos: [0, 0, 0], rot: [0, 0, 0], scale: [1, 1, 1] });

const identityInstance = (): Instance => ({
  ...identityTransform(),
  id: randomUUID().replace(/-/g, '').slice(0, 8),
});

const usage = `Usage:
  geotoy render <path> [options]   Render a composition to an image
  geotoy eval   <path> [options]   Run a composition and print its outputs as JSON

  <path>  Either a directory containing a composition, or a single .geo file
          (treated as the _root source).

Common options:
  --dev                Use localhost services instead of prod
  --backend <url>      Override the backend URL (advanced)
  --token <token>      CLI token; falls back to $GEOTOY_CLI_TOKEN
  --no-prelude         Skip the standard geoscript prelude (default: included)
  --timeout <seconds>  Timeout before failing with diagnostics (default 10, eval 30)
  -h, --help           Show this message

render options:
  -o, --out <file>     Output image path (default: <basename>.<ext>)
  --width <n>          Render width in px (default 800)
  --height <n>         Render height in px (default 800)
  --format <fmt>       png (default) | avif | jpeg
  --quality <n>        Quality 0-100 for avif/jpeg
  --material <mode>    Debug material for all meshes: normal | wireframe | wireframe-xray
  --stdout             Write image to stdout (suppresses progress)

eval options:
  -o, --out <file>     Write the JSON envelope to a file (default: stdout)
  --expr <geoscript>   Evaluate an expression against the composition's root scope
  --samples <n>        Sample callable/path values at N points over t in [0,1] (default 0)
  --meshes <fmt>       Mesh detail: summary (default) | glb | gltf | obj | json
  --meshes-out <file>  Where to write full mesh geometry (default: beside --out, else embedded)
`;

const die = (msg: string, code = 1): never => {
  process.stderr.write(`${msg}\n`);
  process.exit(code);
};

const isDir = (p: string): boolean => {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
};

const readMaybe = (p: string): string | null => {
  try {
    return fs.readFileSync(p, 'utf8');
  } catch {
    return null;
  }
};

const parseJsonFile = (p: string): unknown => {
  const text = fs.readFileSync(p, 'utf8');
  try {
    return JSON.parse(text);
  } catch (err) {
    die(`Failed to parse ${p}: ${err instanceof Error ? err.message : err}`);
  }
};

const buildPayload = (input: string): TransientPayload => {
  const directory = isDir(input);
  let rootSource: string;
  let globalsSource = '';
  let extraNodes: { name: string; source: string }[] = [];
  let tree: TreeDef | null = null;
  let view: ViewDef | undefined;
  let materials: unknown;
  let environment: unknown;
  let preludeEjected: boolean | undefined;

  if (!directory) {
    if (!input.endsWith('.geo')) {
      process.stderr.write(`Warning: ${input} doesn't end in .geo — treating as Geoscript source anyway.\n`);
    }
    rootSource = fs.readFileSync(input, 'utf8');
  } else {
    const mainPath = path.join(input, 'main.geo');
    const treePath = path.join(input, 'tree.json');
    if (fs.existsSync(treePath)) {
      tree = parseJsonFile(treePath) as TreeDef;
      rootSource = '';
    } else if (fs.existsSync(mainPath)) {
      rootSource = fs.readFileSync(mainPath, 'utf8');
    } else {
      die(`${input}: missing main.geo (or tree.json for full override)`);
    }

    const globals = readMaybe(path.join(input, 'globals.geo'));
    if (globals !== null) globalsSource = globals;

    const nodesDir = path.join(input, 'nodes');
    if (fs.existsSync(nodesDir) && fs.statSync(nodesDir).isDirectory()) {
      for (const f of fs.readdirSync(nodesDir).sort()) {
        if (!f.endsWith('.geo')) continue;
        const name = f.slice(0, -'.geo'.length);
        if (name === ROOT_NODE_NAME) {
          die(`nodes/${f}: cannot use reserved name '${ROOT_NODE_NAME}' for a child node`);
        }
        extraNodes.push({ name, source: fs.readFileSync(path.join(nodesDir, f), 'utf8') });
      }
    }

    const viewPath = path.join(input, 'view.json');
    if (fs.existsSync(viewPath)) view = parseJsonFile(viewPath) as ViewDef;

    const matPath = path.join(input, 'materials.json');
    if (fs.existsSync(matPath)) materials = parseJsonFile(matPath);

    const envPath = path.join(input, 'environment.json');
    if (fs.existsSync(envPath)) environment = parseJsonFile(envPath);

    const ejectPath = path.join(input, '.prelude_ejected');
    if (fs.existsSync(ejectPath)) preludeEjected = true;
  }

  if (!tree) {
    const rootId = randomUUID();
    const nodes: Record<string, NodeDef> = {
      [rootId]: {
        id: rootId,
        name: ROOT_NODE_NAME,
        source: rootSource,
        instances: [identityInstance()],
        children: [],
      },
    };
    for (const { name, source } of extraNodes) {
      const id = randomUUID();
      nodes[id] = {
        id,
        name,
        source,
        instances: [identityInstance()],
        children: [],
      };
      nodes[rootId].children.push(id);
    }
    tree = { version: 1, rootId, globalsSource, nodes };
  }

  return {
    tree,
    metadata: {
      view,
      materials,
      preludeEjected,
      environment,
    },
  };
};

const parseArgs = (argv: string[]) => {
  if (argv.length === 0 || argv[0] === '-h' || argv[0] === '--help') {
    process.stdout.write(usage);
    process.exit(0);
  }
  const cmd = argv[0];
  if (cmd !== 'render' && cmd !== 'eval') {
    die(`Unknown command '${cmd}'.\n${usage}`);
  }

  const rest = argv.slice(1);
  const positionals: string[] = [];
  const opts: Record<string, string | boolean> = {};
  const flag = (k: string) => (opts[k] = true);
  const val = (k: string, i: number): number => {
    const v = rest[i + 1];
    if (v === undefined) die(`--${k} requires a value`);
    opts[k] = v;
    return i + 1;
  };

  for (let i = 0; i < rest.length; i++) {
    const a = rest[i];
    switch (a) {
      case '-h':
      case '--help':
        process.stdout.write(usage);
        process.exit(0);
        break;
      case '-o':
      case '--out':
        i = val('out', i);
        break;
      case '--dev':
        flag('dev');
        break;
      case '--backend':
        i = val('backend', i);
        break;
      case '--token':
        i = val('token', i);
        break;
      case '--width':
        i = val('width', i);
        break;
      case '--height':
        i = val('height', i);
        break;
      case '--format':
        i = val('format', i);
        break;
      case '--quality':
        i = val('quality', i);
        break;
      case '--material':
        i = val('material', i);
        break;
      case '--no-prelude':
        flag('no-prelude');
        break;
      case '--timeout':
        i = val('timeout', i);
        break;
      case '--stdout':
        flag('stdout');
        break;
      case '--expr':
        i = val('expr', i);
        break;
      case '--samples':
        i = val('samples', i);
        break;
      case '--meshes':
        i = val('meshes', i);
        break;
      case '--meshes-out':
        i = val('meshes-out', i);
        break;
      default:
        if (a.startsWith('-')) die(`Unknown flag ${a}.\n${usage}`);
        positionals.push(a);
    }
  }
  if (positionals.length !== 1) die(`${cmd} expects exactly one input path.\n${usage}`);
  return { cmd, input: positionals[0], opts };
};

interface Common {
  backend: string;
  token: string;
  timeoutMs: number;
  dev: boolean;
}

const resolveCommon = (opts: Opts, defaultTimeoutSec: number): Common => {
  const dev = !!opts.dev;
  const backend = (opts.backend as string | undefined) ?? (dev ? DEV_BACKEND : PROD_BACKEND);
  const token = (opts.token as string | undefined) ?? process.env.GEOTOY_CLI_TOKEN ?? '';
  if (!token) die('Missing CLI token. Pass --token or set GEOTOY_CLI_TOKEN.');
  const timeoutSec = opts.timeout ? parseFloat(opts.timeout as string) : defaultTimeoutSec;
  if (!Number.isFinite(timeoutSec) || timeoutSec <= 0) die(`Invalid --timeout ${opts.timeout}`);
  return { backend, token, timeoutMs: Math.round(timeoutSec * 1000), dev };
};

const post = async (common: Common, payload: TransientPayload): Promise<Response> => {
  // Backstop the server-side timeout so a stuck backend can't hang the CLI.
  const abort = new AbortController();
  const graceMs = common.timeoutMs + 15_000;
  const fetchTimer = setTimeout(() => abort.abort(), graceMs);
  try {
    const res = await fetch(common.backend, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CLI-Token': common.token },
      body: JSON.stringify(payload),
      signal: abort.signal,
    });
    if (!res.ok) {
      const text = await res.text().catch(() => '');
      die(`Server returned ${res.status} ${res.statusText}\n${text}`);
    }
    return res;
  } catch (err) {
    if (abort.signal.aborted) {
      return die(`Request timed out after ${graceMs / 1000}s with no response from ${common.backend}`);
    }
    return die(`Request failed: ${err instanceof Error ? err.message : err}`);
  } finally {
    clearTimeout(fetchTimer);
  }
};

const baseNameOf = (input: string): string =>
  isDir(input) ? path.basename(path.resolve(input)) : path.basename(input).replace(/\.geo$/, '');

const runRender = async (input: string, opts: Opts) => {
  const common = resolveCommon(opts, 10);
  const format = (opts.format as 'png' | 'avif' | 'jpeg' | undefined) ?? 'png';
  if (!['png', 'avif', 'jpeg'].includes(format)) {
    die(`Invalid --format ${format}. Must be png, avif, or jpeg.`);
  }
  const width = opts.width ? parseInt(opts.width as string, 10) : undefined;
  const height = opts.height ? parseInt(opts.height as string, 10) : undefined;
  const quality = opts.quality ? parseInt(opts.quality as string, 10) : undefined;
  if (width !== undefined && (!Number.isFinite(width) || width < 16)) die(`Invalid --width ${width}`);
  if (height !== undefined && (!Number.isFinite(height) || height < 16)) die(`Invalid --height ${height}`);

  const materialOverride = opts.material as MaterialOverride | undefined;
  if (materialOverride && !['normal', 'wireframe', 'wireframe-xray'].includes(materialOverride)) {
    die(`Invalid --material ${materialOverride}. Must be normal, wireframe, or wireframe-xray.`);
  }

  const payload = buildPayload(input);
  if (opts['no-prelude']) payload.metadata.preludeEjected = true;
  payload.options = {
    format,
    dev: common.dev,
    width,
    height,
    quality,
    timeoutMs: common.timeoutMs,
    materialOverride,
  };

  const toStdout = !!opts.stdout;
  const outPath = (opts.out as string | undefined) ?? `${baseNameOf(input)}.${format}`;
  if (!toStdout) {
    process.stderr.write(`Rendering via ${common.backend} (${format}, ${width ?? 800}x${height ?? 800})...\n`);
  }

  const res = await post(common, payload);
  const buf = Buffer.from(await res.arrayBuffer());
  if (toStdout) {
    process.stdout.write(buf);
  } else {
    fs.writeFileSync(outPath, buf);
    process.stderr.write(`Wrote ${outPath} (${buf.length} bytes)\n`);
  }
};

const MESH_FORMATS: MeshOutputFormat[] = ['summary', 'glb', 'gltf', 'obj', 'json'];

const runEval = async (input: string, opts: Opts) => {
  const common = resolveCommon(opts, 30);

  const meshes = (opts.meshes as MeshOutputFormat | undefined) ?? 'summary';
  if (!MESH_FORMATS.includes(meshes)) {
    die(`Invalid --meshes ${meshes}. Must be one of: ${MESH_FORMATS.join(', ')}.`);
  }
  const samples = opts.samples !== undefined ? parseInt(opts.samples as string, 10) : undefined;
  if (samples !== undefined && (!Number.isFinite(samples) || samples < 0)) die(`Invalid --samples ${opts.samples}`);

  const evalReq: EvalRequest = {
    expr: opts.expr as string | undefined,
    samples,
    meshes,
  };

  const payload = buildPayload(input);
  if (opts['no-prelude']) payload.metadata.preludeEjected = true;
  payload.options = { dev: common.dev, timeoutMs: common.timeoutMs, eval: evalReq };

  const outPath = opts.out as string | undefined;
  process.stderr.write(`Evaluating via ${common.backend}...\n`);
  const res = await post(common, payload);
  const envelope = JSON.parse(await res.text()) as Record<string, any>;

  // Extract full mesh geometry to a sidecar file (base64 fallback stays embedded when there's
  // nowhere to write it — pure-stdout mode with no --meshes-out).
  const md = envelope.meshData;
  if (md && (md.format === 'glb' || md.format === 'gltf' || md.format === 'obj')) {
    const meshesOut =
      (opts['meshes-out'] as string | undefined) ??
      (outPath ? outPath.replace(/\.json$/, '') + '.' + md.format : null);
    if (meshesOut && typeof md.data === 'string') {
      const bytes = md.encoding === 'base64' ? Buffer.from(md.data, 'base64') : Buffer.from(md.data, 'utf8');
      fs.writeFileSync(meshesOut, bytes);
      envelope.meshData = { format: md.format, path: meshesOut, bytes: bytes.length };
      process.stderr.write(`Wrote ${meshesOut} (${bytes.length} bytes)\n`);
    }
  }

  const json = JSON.stringify(envelope, null, 2);
  if (outPath) {
    fs.writeFileSync(outPath, json);
    process.stderr.write(`Wrote ${outPath}\n`);
  } else {
    process.stdout.write(json + '\n');
  }
};

const main = async () => {
  const { cmd, input, opts } = parseArgs(process.argv.slice(2));
  if (cmd === 'eval') {
    await runEval(input, opts);
  } else {
    await runRender(input, opts);
  }
};

main().catch(err => die(err instanceof Error ? err.stack ?? err.message : String(err)));
