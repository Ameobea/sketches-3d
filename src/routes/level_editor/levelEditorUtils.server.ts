import { writeFileSync } from 'fs';

import { dev } from '$app/environment';
import { error } from '@sveltejs/kit';

import { formatLevelJson } from 'src/viz/levelDef/formatLevelJson';
import { readLevelSourceFiles } from 'src/viz/levelDef/levelSourceFiles.server';
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
  let source: ReturnType<typeof readLevelSourceFiles> | null = null;
  try {
    source = readLevelSourceFiles(name);
  } catch (err) {
    const code =
      typeof err === 'object' && err !== null && 'code' in err ? (err as { code?: string }).code : undefined;
    if (code === 'ENOENT') {
      error(404, `Level "${name}" not found`);
    }
    throw err;
  }
  if (!source) error(404, `Level "${name}" not found`);
  const { def, defPath, materialsFilePath, objectsFilePath } = source;

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
