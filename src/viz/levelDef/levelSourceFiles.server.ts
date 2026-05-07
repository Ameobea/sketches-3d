import { existsSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { formatLevelJson } from './formatLevelJson';
import { getLevelDir } from './levelPaths.server';
import type { LevelDefRaw } from './types';

export interface LevelSourceFiles {
  levelDir: string;
  defPath: string;
  materialsFilePath?: string;
  objectsFilePath?: string;
  audioFilePath?: string;
  def: LevelDefRaw;
}

interface ReadLevelSourceFilesOpts {
  syncSchemas?: boolean;
}

const syncSchema = (filePath: string, json: Record<string, unknown>, schemaPath: string): void => {
  if (json.$schema === schemaPath) return;
  json.$schema = schemaPath;
  writeFileSync(filePath, formatLevelJson(json), 'utf-8');
};

/**
 * Reads the raw level files from disk, merges any sidecar files into the def,
 * and auto-discovers geoscript assets from the level-local `geo/` directory.
 */
export const readLevelSourceFiles = (name: string, opts: ReadLevelSourceFilesOpts = {}): LevelSourceFiles => {
  const levelDir = getLevelDir(name);
  const defPath = join(levelDir, 'def.json');
  const def = JSON.parse(readFileSync(defPath, 'utf-8')) as LevelDefRaw;

  if (opts.syncSchemas) {
    syncSchema(defPath, def as Record<string, unknown>, '../schema.json');
  }

  const materialsPath = join(levelDir, 'materials.json');
  const objectsPath = join(levelDir, 'objects.json');
  const audioPath = join(levelDir, 'audio.json');
  const materialsFilePath = existsSync(materialsPath) ? materialsPath : undefined;
  const objectsFilePath = existsSync(objectsPath) ? objectsPath : undefined;
  const audioFilePath = existsSync(audioPath) ? audioPath : undefined;

  if (materialsFilePath) {
    const mats = JSON.parse(readFileSync(materialsFilePath, 'utf-8')) as Record<string, unknown>;
    if (opts.syncSchemas) {
      syncSchema(materialsFilePath, mats, '../materials-schema.json');
    }
    if (mats.textures) def.textures = { ...def.textures, ...mats.textures };
    if (mats.materials) def.materials = { ...def.materials, ...mats.materials };
  }

  if (objectsFilePath) {
    const objs = JSON.parse(readFileSync(objectsFilePath, 'utf-8')) as {
      $schema?: string;
      objects?: LevelDefRaw['objects'];
    };
    if (opts.syncSchemas) {
      syncSchema(objectsFilePath, objs as Record<string, unknown>, '../objects-schema.json');
    }
    const merged = new Map((def.objects ?? []).map(node => [node.id, node]));
    for (const obj of objs.objects ?? []) merged.set(obj.id, obj);
    def.objects = [...merged.values()];
  }

  if (audioFilePath) {
    const audio = JSON.parse(readFileSync(audioFilePath, 'utf-8')) as Record<string, unknown> & {
      sfxDefs?: NonNullable<LevelDefRaw['audio']>['sfxDefs'];
      spatialLoops?: NonNullable<LevelDefRaw['audio']>['spatialLoops'];
    };
    if (opts.syncSchemas) {
      syncSchema(audioFilePath, audio, '../audio-schema.json');
    }
    const existing = def.audio ?? {};
    def.audio = {
      ...existing,
      ...(audio.sfxDefs ? { sfxDefs: { ...(existing.sfxDefs ?? {}), ...audio.sfxDefs } } : {}),
      ...(audio.spatialLoops ? { spatialLoops: audio.spatialLoops } : {}),
    };
  }

  const geoDir = join(levelDir, 'geo');
  if (existsSync(geoDir)) {
    def.assets ??= {};
    // Recursively scan `geo/` for `.geo` files. Files in subdirectories get
    // slash-separated IDs (e.g. `center_fenced_ring/bottom`) and the `file`
    // path mirrors the on-disk relative path under the level dir.
    const walk = (dir: string, relFromGeo: string): void => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        if (entry.isDirectory()) {
          walk(join(dir, entry.name), relFromGeo ? `${relFromGeo}/${entry.name}` : entry.name);
        } else if (entry.isFile() && entry.name.endsWith('.geo')) {
          const stem = entry.name.slice(0, -4);
          const id = relFromGeo ? `${relFromGeo}/${stem}` : stem;
          if (!(id in def.assets!)) {
            def.assets![id] = { type: 'geoscript', file: `geo/${id}.geo` };
          }
        }
      }
    };
    walk(geoDir, '');
  }

  return {
    levelDir,
    defPath,
    materialsFilePath,
    objectsFilePath,
    audioFilePath,
    def,
  };
};
