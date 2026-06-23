import * as THREE from 'three';

// The depth pre-pass renders every opaque mesh with an override material whose vertex stage pins
// `invariant gl_Position` (see depthExactVertex.glsl). For the main color pass to survive the
// LessEqualDepth re-test its gl_Position must be bit-identical, which GLSL only guarantees when
// both shaders declare the qualifier. Stock Three.js material shaders omit it, so
// MeshBasicMaterial / MeshNormalMaterial / LineBasicMaterial / etc. jitter against the pre-pass
// depth. Their gl_Position math already matches the override (all use `#include <project_vertex>`);
// patching the qualifier into the shared ShaderLib sources fixes every instance app-wide.
//
// `standard` and `physical` both back MeshStandardMaterial/MeshPhysicalMaterial depending on
// Three.js version, so both are patched.
const PATCHED_SHADER_IDS = ['basic', 'lambert', 'phong', 'toon', 'matcap', 'standard', 'physical', 'normal'];

let patched = false;

export const ensureGlPositionInvariant = () => {
  if (patched) {
    return;
  }
  patched = true;
  for (const id of PATCHED_SHADER_IDS) {
    const shader = THREE.ShaderLib[id];
    if (shader && !shader.vertexShader.includes('invariant gl_Position')) {
      shader.vertexShader = `invariant gl_Position;\n${shader.vertexShader}`;
    }
  }
};
