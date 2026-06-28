import { existsSync, readdirSync, statSync } from 'fs';
import { join } from 'path';

import { json } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';

import { getAssetsDir } from 'src/viz/levelDef/levelPaths.server';
import type { AssetLibFile, AssetLibFolder } from 'src/viz/levelDef/assetLibTypes';

import { guardDev } from '../../levelEditorUtils.server';

/**
 * Scans `dir` for library materials. A material is either a flat `<name>.json` or the co-located
 * directory form `<name>/<name>.json` (treated as a single entry, not a descendable folder). Each
 * file's `__ASSETS__/…` path is extension-less, matching how `material:` refs are written.
 */
const scanDir = (
  dir: string,
  relFromAssets: string
): { files: AssetLibFile[]; subfolders: AssetLibFolder[] } => {
  const files: AssetLibFile[] = [];
  const subfolders: AssetLibFolder[] = [];
  if (!existsSync(dir)) return { files, subfolders };

  for (const entry of readdirSync(dir).sort()) {
    const entryPath = join(dir, entry);
    const entryRel = `${relFromAssets}/${entry}`;
    if (statSync(entryPath).isDirectory()) {
      if (existsSync(join(entryPath, `${entry}.json`))) {
        files.push({ name: entry, path: `__ASSETS__/${entryRel}` });
      } else {
        const inner = scanDir(entryPath, entryRel);
        if (inner.files.length > 0 || inner.subfolders.length > 0) {
          subfolders.push({ name: entry, files: inner.files, subfolders: inner.subfolders });
        }
      }
    } else if (entry.endsWith('.json')) {
      files.push({ name: entry.slice(0, -5), path: `__ASSETS__/${entryRel.slice(0, -5)}` });
    }
  }

  return { files, subfolders };
};

/** Returns the shared material library tree as JSON. Dev only. */
export const GET: RequestHandler = () => {
  guardDev();
  const { subfolders } = scanDir(join(getAssetsDir(), 'materials'), 'materials');
  return json({ folders: subfolders });
};
