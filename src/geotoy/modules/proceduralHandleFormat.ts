/**
 * Material texture slots reference a texture tab's `render_texture` output through a
 * sentinel handle string, `procedural:<tabId>:<outputName>`, flowing through the same
 * string-handle plumbing as library texture ids. Pure string format — safe to import
 * server-side (the texture registry lives in `proceduralTextures.ts`).
 */
export const PROCEDURAL_HANDLE_PREFIX = 'procedural:';
/** `render_texture_stack` outputs. Stack-ness lives in the handle so the registry can
 *  create the right placeholder kind (`DataArrayTexture`) before the producing tab has
 *  ever run; consumers detect stack-backed slots via `instanceof`. */
export const PROCEDURAL_STACK_HANDLE_PREFIX = 'procedural-stack:';

/** True for both single (`procedural:`) and stack (`procedural-stack:`) handles. */
export const isProceduralHandle = (handle: string): boolean =>
  handle.startsWith(PROCEDURAL_HANDLE_PREFIX) || handle.startsWith(PROCEDURAL_STACK_HANDLE_PREFIX);

export const isStackHandle = (handle: string): boolean => handle.startsWith(PROCEDURAL_STACK_HANDLE_PREFIX);

export const buildProceduralHandle = (tabId: string, output: string, stack = false): string =>
  `${stack ? PROCEDURAL_STACK_HANDLE_PREFIX : PROCEDURAL_HANDLE_PREFIX}${tabId}:${output}`;

/** Output names are free-form and may contain `:`; tab ids can't, so split on the first. */
export const parseProceduralHandle = (
  handle: string
): { tabId: string; output: string; stack: boolean } | null => {
  if (!isProceduralHandle(handle)) return null;
  const stack = isStackHandle(handle);
  const rest = handle.slice((stack ? PROCEDURAL_STACK_HANDLE_PREFIX : PROCEDURAL_HANDLE_PREFIX).length);
  const ix = rest.indexOf(':');
  return ix > 0 ? { tabId: rest.slice(0, ix), output: rest.slice(ix + 1), stack } : null;
};
