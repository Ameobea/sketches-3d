export const isWasmTrap = (err: unknown) =>
  err instanceof Error && (err.name === 'RuntimeError' || err.name === 'WasmPanic');
