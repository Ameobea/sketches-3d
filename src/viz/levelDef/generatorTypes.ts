import type { LevelDefRaw, ObjectDef, ObjectGroupDef, ScenePhysicsDef } from './types';

export interface GeneratorCtx {
  def: LevelDefRaw;
  physics: ScenePhysicsDef;
  params: Record<string, unknown>;
}

export interface GeneratorResult {
  objects: (ObjectDef | ObjectGroupDef)[];
}

export type GeneratorFn = (ctx: GeneratorCtx) => GeneratorResult | Promise<GeneratorResult>;
