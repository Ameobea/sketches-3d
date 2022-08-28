let engine: Promise<typeof import('./wasmComp/engine')> | null = null;

export const getEngine = (): Promise<typeof import('./wasmComp/engine')> => {
  if (engine === null) {
    engine = import('./wasmComp/engine').then(async engineMod => {
      await engineMod.default();
      return engineMod;
    });
  }
  return engine;
};
