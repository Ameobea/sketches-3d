import { isAbsolute, join, resolve } from 'path';

const resolveConfiguredPath = (configuredPath: string): string =>
  isAbsolute(configuredPath) ? configuredPath : resolve(process.cwd(), configuredPath);

/**
 * Root directory containing level subdirectories like `<level>/def.json`.
 *
 * Defaults to the source tree for local development, but can be overridden at
 * runtime for container builds that copy levels elsewhere.
 */
export const getLevelsDir = (): string => {
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
