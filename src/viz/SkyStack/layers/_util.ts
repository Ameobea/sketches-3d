/**
 * Layer-author convention: all uniform names and helper-function names in
 * per-instance GLSL are suffixed with `_$ID`. The `$ID` token is illegal in
 * GLSL identifiers, so any missed substitution becomes an immediate shader
 * compile error instead of silently mislinking two instances.
 */
export const resolveId = (glsl: string, id: string): string => glsl.replace(/\$ID/g, id);
