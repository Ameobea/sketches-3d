import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { dev } from '$app/environment';
import { error } from '@sveltejs/kit';

import type { LevelDefRaw } from 'src/viz/levelDef/types';

export const readLevel = (name: string): { filePath: string; levelDef: LevelDefRaw } => {
  const filePath = join(process.cwd(), 'src', 'levels', name, 'def.json');
  try {
    return { filePath, levelDef: JSON.parse(readFileSync(filePath, 'utf-8')) };
  } catch {
    error(404, `Level "${name}" not found`);
  }
};

export const writeLevel = (filePath: string, levelDef: LevelDefRaw) => {
  writeFileSync(filePath, JSON.stringify(levelDef, null, 2) + '\n');
};

export const guardDev = () => {
  if (!dev) error(403, 'Level editor is disabled in production');
};

export const validateName = (name: string | undefined): string => {
  if (!name || !/^[a-z0-9_]+$/i.test(name)) error(400, 'Invalid level name');
  return name;
};
