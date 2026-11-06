import { isAbsolute, join, resolve, sep } from 'path';

import { error } from '@sveltejs/kit';

const resolveConfiguredPath = (configuredPath: string): string =>
  isAbsolute(configuredPath) ? configuredPath : resolve(process.cwd(), configuredPath);

/**
 * Root directory containing level subdirectories like `<level>/def.json`.
 *
 * Defaults to the source tree for local development, but can be overridden at
 * runtime for container builds that copy levels elsewhere.
 */
const getLevelsDir = (): string => {
  const configuredDir = process.env.LEVELS_DIR;
  return configuredDir ? resolveConfiguredPath(configuredDir) : join(process.cwd(), 'src', 'levels');
};

export const getLevelDir = (name: string): string => join(getLevelsDir(), name);

/**
 * Root directory for shared assets (meshes, etc.) that can be referenced
 * across multiple levels using the `__ASSETS__/` path prefix.
 *
 * Defaults to the source tree for local development, but can be overridden at
 * runtime for container builds that copy assets elsewhere.
 */
export const getAssetsDir = (): string => {
  const configuredDir = process.env.ASSETS_DIR;
  return configuredDir ? resolveConfiguredPath(configuredDir) : join(process.cwd(), 'src', 'assets');
};

export const resolveInside = (root: string, rel: string): string => {
  const base = resolve(root);
  const path = resolve(base, rel);
  if (!path.startsWith(base + sep)) error(400, `Path escapes ${base}: ${rel}`);
  return path;
};

/** Resolves a `file` reference: `__ASSETS__/` → shared assets dir, else relative to `baseDir`. */
export const resolveFileRef = (baseDir: string, file: string): string =>
  file.startsWith('__ASSETS__/')
    ? resolveInside(getAssetsDir(), file.slice('__ASSETS__/'.length))
    : resolveInside(baseDir, file);
