import { existsSync, readdirSync, statSync } from 'fs';
import { join } from 'path';

import { json } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';

import { getAssetsDir } from 'src/viz/levelDef/levelPaths.server';
import type { AssetLibFile, AssetLibFolder } from 'src/viz/levelDef/assetLibTypes';

import { guardDev } from '../../levelEditorUtils.server';

/**
 * Recursively scans `dir` and builds a tree of AssetLibFolders.
 * `relFromAssets` is the path from the assets root to `dir` (e.g. "meshes/spinners"),
 * used to construct the `__ASSETS__/…` path for each file.
 */
const scanDir = (dir: string, relFromAssets: string): AssetLibFolder[] => {
  const folders: AssetLibFolder[] = [];
  if (!existsSync(dir)) return folders;

  for (const entry of readdirSync(dir).sort()) {
    const entryPath = join(dir, entry);
    if (!statSync(entryPath).isDirectory()) continue;

    const entryRel = `${relFromAssets}/${entry}`;

    const files: AssetLibFile[] = readdirSync(entryPath)
      .filter(f => f.endsWith('.geo'))
      .sort()
      .map(f => ({
        name: f.slice(0, -4),
        path: `__ASSETS__/${entryRel}/${f}`,
      }));

    const subfolders = scanDir(entryPath, entryRel);

    if (files.length > 0 || subfolders.length > 0) {
      folders.push({ name: entry, files, subfolders });
    }
  }

  return folders;
};

/**
 * Returns the asset library mesh tree as JSON.
 *
 * The "meshes" top-level directory is skipped in the display tree — its
 * subdirectories are returned as the top-level folders — but the full path
 * (including "meshes") is preserved in each file's `path` field so that
 * `__ASSETS__/meshes/spinners/gear1.geo` resolves correctly on the server.
 *
 * Dev only.
 */
export const GET: RequestHandler = () => {
  guardDev();

  const assetsDir = getAssetsDir();
  const meshesDir = join(assetsDir, 'meshes');

  // Scan inside "meshes/" but root the relative paths at "meshes" so that
  // the constructed __ASSETS__ paths are correct.
  const folders = scanDir(meshesDir, 'meshes');

  return json({ folders });
};
