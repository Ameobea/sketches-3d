export const isWasmTrap = (err: unknown) =>
  err instanceof Error &&
  (err.name === 'RuntimeError' ||
    err.name === 'WasmPanic' ||
    // JS stack overflow unwinds wasm frames without restoring its shadow stack pointer
    (err.name === 'RangeError' && /call stack/i.test(err.message)));
