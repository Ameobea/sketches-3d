import { existsSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { dev } from '$app/environment';
import { error } from '@sveltejs/kit';

import { formatLevelJson } from 'src/viz/levelDef/formatLevelJson';
import { getLevelDir } from 'src/viz/levelDef/levelPaths.server';
import type { LevelDefRaw } from 'src/viz/levelDef/types';

export const guardDev = () => {
  if (!dev) error(403, 'Level editor is disabled in production');
};

export const validateName = (name: string | undefined): string => {
  if (!name || !/^[a-z0-9_]+$/i.test(name)) error(400, 'Invalid level name');
  return name;
};

/**
 * A handle to an open level definition. `def` is the fully-merged view of the
 * level — whether the level uses a monolithic `def.json` or has materials /
 * objects split into separate files is transparent to callers. Mutate `def`
 * freely and call `save()` to persist.
 */
export interface LevelStore {
  def: LevelDefRaw;
  save(): void;
}

/**
 * Opens a level by name and returns a `LevelStore`.
 *
 * Merges any optional `materials.json` and `objects.json` sidecar files into
 * `def` and auto-discovers `.geo` files from the level's `geo/` subdirectory
 * (injecting them as geoscript assets if not already declared in `def.json`).
 *
 * `save()` writes mutations back to whichever file(s) own each section.
 */
export const openLevel = (name: string): LevelStore => {
  const levelDir = getLevelDir(name);
  const defPath = join(levelDir, 'def.json');

  let def: LevelDefRaw;
  try {
    def = JSON.parse(readFileSync(defPath, 'utf-8'));
  } catch {
    error(404, `Level "${name}" not found`);
  }

  const materialsPath = join(levelDir, 'materials.json');
  const objectsPath = join(levelDir, 'objects.json');
  const materialsFilePath = existsSync(materialsPath) ? materialsPath : undefined;
  const objectsFilePath = existsSync(objectsPath) ? objectsPath : undefined;

  // Merge materials.json over def.json (external wins on conflict)
  if (materialsFilePath) {
    const mats = JSON.parse(readFileSync(materialsFilePath, 'utf-8'));
    if (mats.textures) def.textures = { ...def.textures, ...mats.textures };
    if (mats.materials) def.materials = { ...def.materials, ...mats.materials };
  }

  // Merge objects.json over def.json objects when present.
  // objects.json entries win on ID conflict; def.json entries not present in objects.json
  // are retained (e.g. generator anchor groups that are structural, not editor-placed).
  if (objectsFilePath) {
    const objs = JSON.parse(readFileSync(objectsFilePath, 'utf-8'));
    const merged = new Map((def.objects ?? []).map(n => [n.id, n]));
    for (const obj of objs.objects ?? []) merged.set(obj.id, obj);
    def.objects = [...merged.values()];
  }

  // Auto-discover *.geo files from the geo/ subdirectory
  const geoDir = join(levelDir, 'geo');
  if (existsSync(geoDir)) {
    for (const file of readdirSync(geoDir).filter(f => f.endsWith('.geo'))) {
      const id = file.slice(0, -4);
      if (!(id in def.assets)) {
        def.assets[id] = { type: 'geoscript', file: `geo/${file}` };
      }
    }
  }

  const save = () => {
    const defToWrite = { ...def };

    if (materialsFilePath) {
      writeFileSync(
        materialsFilePath,
        formatLevelJson({
          $schema: '../materials-schema.json',
          textures: defToWrite.textures,
          materials: defToWrite.materials,
        })
      );
      delete defToWrite.textures;
      delete defToWrite.materials;
    }

    if (objectsFilePath) {
      writeFileSync(
        objectsFilePath,
        formatLevelJson({ $schema: '../objects-schema.json', objects: defToWrite.objects })
      );
      defToWrite.objects = [];
    }

    writeFileSync(defPath, formatLevelJson(defToWrite));
  };

  return { def, save };
};
