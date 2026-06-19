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

interface RenderOptions {
  format?: 'png' | 'avif' | 'jpeg';
  width?: number;
  height?: number;
  quality?: number;
  dev?: boolean;
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

const identityTransform = (): Transform3 => ({ pos: [0, 0, 0], rot: [0, 0, 0], scale: [1, 1, 1] });

const identityInstance = (): Instance => ({
  ...identityTransform(),
  id: randomUUID().replace(/-/g, '').slice(0, 8),
});

const usage = `Usage: geotoy render <path> [options]

  <path>  Either a directory containing a composition, or a single .geo file
          (treated as the _root source).

Options:
  -o, --out <file>     Output image path (default: <basename>.<ext>)
  --dev                Use localhost services instead of prod
  --backend <url>      Override the backend URL (advanced)
  --token <token>      CLI token; falls back to $GEOTOY_CLI_TOKEN
  --width <n>          Render width in px (default 800)
  --height <n>         Render height in px (default 800)
  --format <fmt>       png (default) | avif | jpeg
  --quality <n>        Quality 0-100 for avif/jpeg
  --no-prelude         Skip the standard geoscript prelude (default: included)
  --stdout             Write image to stdout (suppresses progress)
  -h, --help           Show this message
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
  if (cmd !== 'render') {
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
      case '--no-prelude':
        flag('no-prelude');
        break;
      case '--stdout':
        flag('stdout');
        break;
      default:
        if (a.startsWith('-')) die(`Unknown flag ${a}.\n${usage}`);
        positionals.push(a);
    }
  }
  if (positionals.length !== 1) die(`render expects exactly one input path.\n${usage}`);
  return { input: positionals[0], opts };
};

const main = async () => {
  const argv = process.argv.slice(2);
  const { input, opts } = parseArgs(argv);

  const dev = !!opts.dev;
  const backend = (opts.backend as string | undefined) ?? (dev ? DEV_BACKEND : PROD_BACKEND);
  const token = (opts.token as string | undefined) ?? process.env.GEOTOY_CLI_TOKEN ?? '';
  if (!token) {
    die('Missing CLI token. Pass --token or set GEOTOY_CLI_TOKEN.');
  }

  const format = (opts.format as 'png' | 'avif' | 'jpeg' | undefined) ?? 'png';
  if (!['png', 'avif', 'jpeg'].includes(format)) {
    die(`Invalid --format ${format}. Must be png, avif, or jpeg.`);
  }
  const width = opts.width ? parseInt(opts.width as string, 10) : undefined;
  const height = opts.height ? parseInt(opts.height as string, 10) : undefined;
  const quality = opts.quality ? parseInt(opts.quality as string, 10) : undefined;
  if (width !== undefined && (!Number.isFinite(width) || width < 16)) die(`Invalid --width ${width}`);
  if (height !== undefined && (!Number.isFinite(height) || height < 16)) die(`Invalid --height ${height}`);

  const payload = buildPayload(input);
  if (opts['no-prelude']) payload.metadata.preludeEjected = true;
  payload.options = { format, dev, width, height, quality };

  const toStdout = !!opts.stdout;
  const baseName = isDir(input)
    ? path.basename(path.resolve(input))
    : path.basename(input).replace(/\.geo$/, '');
  const defaultOut = `${baseName}.${format}`;
  const outPath = (opts.out as string | undefined) ?? defaultOut;

  if (!toStdout) {
    process.stderr.write(`Rendering via ${backend} (${format}, ${width ?? 800}x${height ?? 800})...\n`);
  }

  let res: Response;
  try {
    res = await fetch(backend, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CLI-Token': token },
      body: JSON.stringify(payload),
    });
  } catch (err) {
    die(`Request failed: ${err instanceof Error ? err.message : err}`);
  }

  if (!res.ok) {
    const text = await res.text().catch(() => '');
    die(`Server returned ${res.status} ${res.statusText}\n${text}`);
  }

  const buf = Buffer.from(await res.arrayBuffer());
  if (toStdout) {
    process.stdout.write(buf);
  } else {
    fs.writeFileSync(outPath, buf);
    process.stderr.write(`Wrote ${outPath} (${buf.length} bytes)\n`);
  }
};

main().catch(err => die(err instanceof Error ? err.stack ?? err.message : String(err)));
