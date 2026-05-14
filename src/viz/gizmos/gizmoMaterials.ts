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
  ACTIVE_BRIGHTEN: 2.0,
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
uniform float uHovered;
uniform float uActive;
uniform float uDimmed;
uniform float uShadeMin;
uniform float uEdgeOutline;

varying vec3 vNormalView;
varying vec3 vViewPos;

void main() {
  vec3 N = normalize(vNormalView);
  vec3 V = normalize(vViewPos);
  float ndv = abs(dot(N, V));

  // View-angle shading + glancing-edge alpha fade.
  float shade = mix(uShadeMin, 1.0, ndv);
  float edge = smoothstep(0.0, fwidth(ndv) * 2.0 + 0.02, ndv);

  vec3 col = uColor * shade;

  // fwidth(N) spikes at face seams (flat normals only) — free edge darkening.
  if (uEdgeOutline > 0.5) {
    float dNorm = length(fwidth(N));
    float edgeMask = clamp(dNorm * 3.0, 0.0, 1.0);
    col = mix(col, col * 0.18, edgeMask);
  }

  if (uActive > 0.5) {
    col = mix(col, vec3(1.0), 0.55) * ${COMMON_DEFINES.ACTIVE_BRIGHTEN.toFixed(2)};
  } else if (uHovered > 0.5) {
    col = mix(col, vec3(1.0), 0.35) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
  }
  if (uDimmed > 0.5) col = mix(col, vec3(dot(col, vec3(0.299, 0.587, 0.114))), ${COMMON_DEFINES.INACTIVE_DESATURATE.toFixed(2)}) * 0.6;

  gl_FragColor = vec4(col * edge, edge);
}
`;

export interface AxisMaterialOpts {
  color: THREE.ColorRepresentation;
  /** ndv=0 shade floor. Default 0.7; ~0.4 for flat-shaded geometry. */
  shadeMin?: number;
  /** Only meaningful for flat-shaded geometry. */
  edgeOutline?: boolean;
}

export const buildAxisMaterial = (opts: AxisMaterialOpts): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    name: 'GizmoAxis',
    vertexShader: AXIS_VERT,
    fragmentShader: AXIS_FRAG,
    uniforms: {
      uColor: { value: new THREE.Color(opts.color) },
      uHovered: { value: 0 },
      uActive: { value: 0 },
      uDimmed: { value: 0 },
      uShadeMin: { value: opts.shadeMin ?? 0.7 },
      uEdgeOutline: { value: opts.edgeOutline ? 1 : 0 },
    },
    transparent: true,
    premultipliedAlpha: true,
    depthTest: false,
    depthWrite: false,
    side: THREE.DoubleSide,
  });

// Arrowhead — view-space billboard triangle with SDF outline ------------------

const ARROWHEAD_VERT = /* glsl */ `
varying vec2 vLocal;

void main() {
  // axisV's length carries the gizmo's autoScale; mirror it onto rightV to preserve aspect.
  vec3 axisV = (modelViewMatrix * vec4(0.0, 1.0, 0.0, 0.0)).xyz;
  vec3 originV = (modelViewMatrix * vec4(0.0, 0.0, 0.0, 1.0)).xyz;

  float axisLen = length(axisV);
  vec3 axisNorm = axisLen > 1e-6 ? axisV / axisLen : vec3(0.0, 1.0, 0.0);

  vec3 toCam = -originV;
  float toCamLen = length(toCam);
  vec3 viewDir = toCamLen > 1e-6 ? toCam / toCamLen : vec3(0.0, 0.0, 1.0);

  // Fall back to screen-X when axis ‖ view, otherwise we'd divide by zero.
  vec3 rightCross = cross(axisNorm, viewDir);
  float rightLen = length(rightCross);
  vec3 rightDir = rightLen > 1e-4 ? rightCross / rightLen : vec3(1.0, 0.0, 0.0);

  vec3 rightV = rightDir * axisLen;

  vec3 viewPos = originV + position.y * axisV + position.x * rightV;
  vLocal = position.xy;
  gl_Position = projectionMatrix * vec4(viewPos, 1.0);
}
`;

const ARROWHEAD_FRAG = /* glsl */ `
precision highp float;

uniform vec3 uColor;
uniform float uHovered;
uniform float uActive;
uniform float uDimmed;
uniform float uFillAlpha;
uniform float uHalfWidth;
uniform float uHeight;
uniform float uOutlineInset;

varying vec2 vLocal;

// Inigo Quilez's signed-triangle distance (negative inside).
float sdTriangle(vec2 p, vec2 a, vec2 b, vec2 c) {
  vec2 e0 = b - a;
  vec2 e1 = c - b;
  vec2 e2 = a - c;
  vec2 v0 = p - a;
  vec2 v1 = p - b;
  vec2 v2 = p - c;
  vec2 pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
  vec2 pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
  vec2 pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
  float s = sign(e0.x * e2.y - e0.y * e2.x);
  vec2 d = min(min(vec2(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
                   vec2(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
                   vec2(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)));
  return -sqrt(d.x) * sign(d.y);
}

void main() {
  float sd = sdTriangle(vLocal, vec2(-uHalfWidth, 0.0), vec2(uHalfWidth, 0.0), vec2(0.0, uHeight));
  // fwidth(vLocal), not fwidth(sd): the SDF gradient is discontinuous at the
  // triangle's vertices and produces scintillating outline noise near corners.
  float fw = length(fwidth(vLocal));

  float coverage = 1.0 - smoothstep(-fw, fw, sd);
  if (coverage < 0.001) discard;

  float bodyFill = 1.0 - smoothstep(-uOutlineInset - fw, -uOutlineInset + fw, sd);

  float outlineDist = abs(sd + uOutlineInset);
  float outline = 1.0 - smoothstep(0.0, fw * 2.5, outlineDist);
  outline *= coverage;

  vec3 col = uColor;
  col = mix(col * 0.55, col, bodyFill);
  col = mix(col, vec3(1.0), outline * 0.7);

  float alpha = max(bodyFill * uFillAlpha, outline);

  if (uActive > 0.5) {
    col = mix(col, vec3(1.0), 0.55) * ${COMMON_DEFINES.ACTIVE_BRIGHTEN.toFixed(2)};
    alpha = max(alpha, bodyFill * 0.9);
  } else if (uHovered > 0.5) {
    col = mix(col, vec3(1.0), 0.35) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
    alpha = max(alpha, bodyFill * 0.8);
  }
  if (uDimmed > 0.5) col = mix(col, vec3(dot(col, vec3(0.299, 0.587, 0.114))), ${COMMON_DEFINES.INACTIVE_DESATURATE.toFixed(2)}) * 0.6;

  gl_FragColor = vec4(col * alpha, alpha);
}
`;

export interface ArrowheadMaterialOpts {
  color: THREE.ColorRepresentation;
  /** Must match the geometry's ±x extent. */
  halfWidth?: number;
  /** Must match the geometry's y extent. */
  height?: number;
  fillAlpha?: number;
  outlineInset?: number;
}

export const buildArrowheadMaterial = (opts: ArrowheadMaterialOpts): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    name: 'GizmoArrowhead',
    vertexShader: ARROWHEAD_VERT,
    fragmentShader: ARROWHEAD_FRAG,
    uniforms: {
      uColor: { value: new THREE.Color(opts.color) },
      uHovered: { value: 0 },
      uActive: { value: 0 },
      uDimmed: { value: 0 },
      uFillAlpha: { value: opts.fillAlpha ?? 0.6 },
      uHalfWidth: { value: opts.halfWidth ?? 0.06 },
      uHeight: { value: opts.height ?? 0.18 },
      uOutlineInset: { value: opts.outlineInset ?? 0.009 },
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
uniform float uActive;
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
  if (uActive > 0.5) {
    col = mix(col, vec3(1.0), 0.55) * ${COMMON_DEFINES.ACTIVE_BRIGHTEN.toFixed(2)};
  } else if (uHovered > 0.5) {
    col = mix(col, vec3(1.0), 0.35) * ${COMMON_DEFINES.HOVER_BRIGHTEN.toFixed(2)};
  }
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
      uActive: { value: 0 },
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
uniform float uActive;
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

  if (uActive > 0.5) {
    col = mix(col, vec3(1.0), 0.6) * ${COMMON_DEFINES.ACTIVE_BRIGHTEN.toFixed(2)};
    alpha = max(alpha, fill * 0.85);
  } else if (uHovered > 0.5) {
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
      uActive: { value: 0 },
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
