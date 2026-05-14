import * as THREE from 'three';

/**
 * ShaderMaterials for the custom gizmo.  Axis (shafts/arrows), Ring (rotation),
 * and Plane (translate/scale planar handles).  Each `build*` returns a fresh
 * material so per-handle hover state doesn't cross-talk.  All use premultiplied
 * alpha (required for MSAA-target compositing — see `overlayMSAA.ts`) and have
 * depth test/write disabled so handles never inconsistently occlude each other.
 */

const COMMON_DEFINES = {
  HOVER_BRIGHTEN: 1.55,
  INACTIVE_DESATURATE: 0.65,
};

// Axis (shaft + arrowhead) -----------------------------------------------------

const AXIS_VERT = /* glsl */ `
varying vec3 vNormalView;
varying vec3 vViewPos;

void main() {
  vec4 mvPos = modelViewMatrix * vec4(position, 1.0);
  vNormalView = normalize(normalMatrix * normal);
  vViewPos = -mvPos.xyz; // fragment → camera, view space
  gl_Position = projectionMatrix * mvPos;
}
`;

const AXIS_FRAG = /* glsl */ `
precision highp float;

uniform vec3 uColor;
uniform float uHovered; // 0 / 1
uniform float uDimmed;  // 0 / 1, set when the handle isn't part of the active axis lock

varying vec3 vNormalView;
varying vec3 vViewPos;

void main() {
  vec3 N = normalize(vNormalView);
  vec3 V = normalize(vViewPos);
  float ndv = abs(dot(N, V));

  // View-angle shading + glancing-edge alpha fade — gives a soft depth cue and
  // resolves single-pixel edges cleanly when MSAA samples=4 isn't enough.
  float shade = mix(0.7, 1.0, ndv);
  float edge = smoothstep(0.0, fwidth(ndv) * 2.0 + 0.02, ndv);

  vec3 col = uColor * shade;
  if (uHovered > 0.5) col = mix(col, vec3(1.0), 0.35) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
  if (uDimmed > 0.5) col = mix(col, vec3(dot(col, vec3(0.299, 0.587, 0.114))), ${COMMON_DEFINES.INACTIVE_DESATURATE.toFixed(2)}) * 0.6;

  gl_FragColor = vec4(col * edge, edge);
}
`;

export interface AxisMaterialOpts {
  color: THREE.ColorRepresentation;
}

export const buildAxisMaterial = (opts: AxisMaterialOpts): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    name: 'GizmoAxis',
    vertexShader: AXIS_VERT,
    fragmentShader: AXIS_FRAG,
    uniforms: {
      uColor: { value: new THREE.Color(opts.color) },
      uHovered: { value: 0 },
      uDimmed: { value: 0 },
    },
    transparent: true,
    premultipliedAlpha: true,
    depthTest: false,
    depthWrite: false,
    side: THREE.DoubleSide,
  });

// Ring (rotation handles) ------------------------------------------------------

const RING_VERT = /* glsl */ `
varying vec2 vLocal; // fragment position in the ring's local XY
varying vec3 vNormalView;
varying vec3 vViewPos;

void main() {
  vLocal = position.xy;
  vec4 mvPos = modelViewMatrix * vec4(position, 1.0);
  vNormalView = normalize(normalMatrix * normal);
  vViewPos = -mvPos.xyz;
  gl_Position = projectionMatrix * mvPos;
}
`;

const RING_FRAG = /* glsl */ `
precision highp float;

uniform vec3 uColor;
uniform float uHovered;
uniform float uDimmed;
uniform float uInnerRadius;
uniform float uOuterRadius;
uniform float uTickCount;

varying vec2 vLocal;
varying vec3 vNormalView;
varying vec3 vViewPos;

void main() {
  float r = length(vLocal);
  float fw = fwidth(r);
  // Annular SDF — in-band when r is between inner and outer, with smooth edges.
  float outerMask = 1.0 - smoothstep(uOuterRadius - fw, uOuterRadius + fw, r);
  float innerMask = smoothstep(uInnerRadius - fw, uInnerRadius + fw, r);
  float band = outerMask * innerMask;
  if (band < 0.001) discard;

  float angle = atan(vLocal.y, vLocal.x);
  float ticks = abs(fract(angle * uTickCount / (2.0 * 3.14159265)) - 0.5) * 2.0;
  float tickWidth = fwidth(angle * uTickCount / (2.0 * 3.14159265)) * 2.0;
  float tickMask = smoothstep(0.5 - tickWidth, 0.5 + tickWidth, ticks);
  float tickShade = mix(0.7, 1.0, tickMask);

  // Edge-on view fade — softens to a hint instead of a shimmering 1-px band.
  vec3 V = normalize(vViewPos);
  vec3 N = normalize(vNormalView);
  float ndv = abs(dot(N, V));
  float viewFade = smoothstep(0.0, 0.25, ndv);

  vec3 col = uColor * tickShade;
  if (uHovered > 0.5) col = mix(col, vec3(1.0), 0.35) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
  if (uDimmed > 0.5) col = mix(col, vec3(dot(col, vec3(0.299, 0.587, 0.114))), ${COMMON_DEFINES.INACTIVE_DESATURATE.toFixed(2)}) * 0.6;

  float a = band * viewFade;
  gl_FragColor = vec4(col * a, a);
}
`;

export interface RingMaterialOpts {
  color: THREE.ColorRepresentation;
  innerRadius: number;
  outerRadius: number;
  tickCount?: number;
}

export const buildRingMaterial = (opts: RingMaterialOpts): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    name: 'GizmoRing',
    vertexShader: RING_VERT,
    fragmentShader: RING_FRAG,
    uniforms: {
      uColor: { value: new THREE.Color(opts.color) },
      uHovered: { value: 0 },
      uDimmed: { value: 0 },
      uInnerRadius: { value: opts.innerRadius },
      uOuterRadius: { value: opts.outerRadius },
      uTickCount: { value: opts.tickCount ?? 24 },
    },
    transparent: true,
    premultipliedAlpha: true,
    depthTest: false,
    depthWrite: false,
    side: THREE.DoubleSide,
  });

// Plane (translate / scale plane handles) --------------------------------------

const PLANE_VERT = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv * 2.0 - 1.0;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const PLANE_FRAG = /* glsl */ `
precision highp float;

uniform vec3 uColor;
uniform float uHovered;
uniform float uDimmed;
uniform float uCornerRadius;

varying vec2 vUv;

float sdRoundBox(vec2 p, vec2 b, float r) {
  vec2 q = abs(p) - b + vec2(r);
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
  float sd = sdRoundBox(vUv, vec2(0.9), uCornerRadius);
  float fw = fwidth(sd);
  float fill = 1.0 - smoothstep(-fw, fw, sd);
  if (fill < 0.001) discard;

  float outlineDist = abs(sd + 0.05);
  float outline = 1.0 - smoothstep(0.0, fw * 2.5, outlineDist);

  vec3 col = uColor;
  col = mix(col * 0.5, col, fill);
  col = mix(col, vec3(1.0), outline * 0.6);
  float alpha = max(fill * 0.35, outline);

  if (uHovered > 0.5) {
    col = mix(col, vec3(1.0), 0.4) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
    alpha = max(alpha, fill * 0.7);
  }
  if (uDimmed > 0.5) {
    col = mix(col, vec3(dot(col, vec3(0.299, 0.587, 0.114))), ${COMMON_DEFINES.INACTIVE_DESATURATE.toFixed(2)}) * 0.6;
  }

  gl_FragColor = vec4(col * alpha, alpha);
}
`;

export interface PlaneMaterialOpts {
  color: THREE.ColorRepresentation;
  cornerRadius?: number;
}

export const buildPlaneMaterial = (opts: PlaneMaterialOpts): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    name: 'GizmoPlane',
    vertexShader: PLANE_VERT,
    fragmentShader: PLANE_FRAG,
    uniforms: {
      uColor: { value: new THREE.Color(opts.color) },
      uHovered: { value: 0 },
      uDimmed: { value: 0 },
      uCornerRadius: { value: opts.cornerRadius ?? 0.15 },
    },
    transparent: true,
    premultipliedAlpha: true,
    depthTest: false,
    depthWrite: false,
    side: THREE.DoubleSide,
  });

/** Invisible material for hit-test pickers. */
export const buildPickerMaterial = (): THREE.MeshBasicMaterial =>
  new THREE.MeshBasicMaterial({ visible: false, depthTest: false, depthWrite: false });

export const AXIS_COLORS = {
  x: 0xff5566,
  y: 0x66dd66,
  z: 0x5588ff,
} as const;
