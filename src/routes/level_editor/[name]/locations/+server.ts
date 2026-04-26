import { existsSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { error, json, type RequestHandler } from '@sveltejs/kit';

import { formatLevelJson } from 'src/viz/levelDef/formatLevelJson';
import { getLevelDir } from 'src/viz/levelDef/levelPaths.server';
import {
  EditorBookmarkSchema,
  LocationsFileSchema,
  type EditorBookmark,
  type LocationsFile,
} from 'src/viz/levelDef/types';
import { guardDev, validateName } from '../../levelEditorUtils.server';

const LOCATIONS_SCHEMA_REF = '../locations-schema.json';

const getLocationsPath = (name: string) => join(getLevelDir(name), 'locations.json');

const readLocations = (name: string): LocationsFile => {
  const path = getLocationsPath(name);
  if (!existsSync(path)) return { $schema: LOCATIONS_SCHEMA_REF };
  const raw = JSON.parse(readFileSync(path, 'utf-8'));
  const parsed = LocationsFileSchema.safeParse(raw);
  if (!parsed.success) {
    error(500, `Invalid locations.json for "${name}": ${parsed.error.message}`);
  }
  return parsed.data;
};

const writeLocations = (name: string, file: LocationsFile) => {
  const path = getLocationsPath(name);
  const out: LocationsFile = { $schema: LOCATIONS_SCHEMA_REF, ...file };
  writeFileSync(path, formatLevelJson(out));
};

export const GET: RequestHandler = async ({ params }) => {
  guardDev();
  const name = validateName(params.name);
  return json(readLocations(name));
};

/** Upsert a single bookmark by slot. */
export const PUT: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = await request.json();
  const parsed = EditorBookmarkSchema.safeParse(body);
  if (!parsed.success) error(400, `Invalid bookmark: ${parsed.error.message}`);
  const bookmark: EditorBookmark = parsed.data;

  const locations = readLocations(name);
  const existing = locations.editor_bookmarks ?? [];
  const idx = existing.findIndex(b => b.slot === bookmark.slot);
  if (idx === -1) existing.push(bookmark);
  else existing[idx] = bookmark;
  existing.sort((a, b) => a.slot - b.slot);

  writeLocations(name, { ...locations, editor_bookmarks: existing });
  return json({ ok: true });
};
