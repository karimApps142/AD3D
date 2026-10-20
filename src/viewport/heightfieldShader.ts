// The realistic Squama preview material: Lambert shading with flat per-triangle normals
// (screen-space derivatives, §12.2) and exact cast shadows from a ray march over the per-cell
// planes (heightfield.ts; the GLSL functions below are line-by-line ports of lookupCell, cellExit,
// planeAt and traceShadow). No shadow map, so no acne or peter-panning at grazing angles.

import * as THREE from 'three';
import {
  type CellPlanes,
  layoutCode,
  TRACE_BIAS,
  TRACE_EPS,
  TRACE_MAX_STEPS,
  TRACE_WALL_START,
} from './heightfield';

const f = (x: number) => (Number.isInteger(x) ? x.toFixed(1) : String(x));

const vertexShader = /* glsl */ `
varying vec3 vPlate;
varying vec3 vWorld;
void main() {
  vPlate = position;
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  gl_Position = projectionMatrix * viewMatrix * world;
}
`;

const fragmentShader = /* glsl */ `
precision highp float;
precision highp int;
precision highp sampler2D;

#define MAX_STEPS ${TRACE_MAX_STEPS}
#define TRACE_EPS ${f(TRACE_EPS)}
#define WALL_START ${f(TRACE_WALL_START)}
#define BIAS ${f(TRACE_BIAS)}
#define SQRT3_2 0.86602540378443864676
#define PI 3.14159265358979323846

uniform sampler2D uPlanes;     // RGBA32F: (w at centre, dw/du, dw/dv, valid)
uniform ivec2 uGrid;           // texture columns, rows
uniform int uLayout;           // 0 square, 1 pointy hex, 2 flat hex
uniform float uCell;
uniform vec2 uPlate;           // plate width, height (mm)
uniform float uZMax;
uniform bool uHasPlanes;

uniform int uLightKind;        // 0 directional, 1 point
uniform vec3 uLightDir;        // unit, toward the light (directional)
uniform vec3 uLightPos;        // plate space (point)
uniform float uRefDist2;       // squared lamp distance at which uDirect applies (point)
uniform float uDirect;         // lamp irradiance at normal incidence
uniform float uAmbient;        // room irradiance
uniform vec3 uAlbedo;          // linear
uniform float uShadowStrength; // 1 = full cast shadows, 0 = none (mirrors the key light's shadow.intensity)

varying vec3 vPlate;
varying vec3 vWorld;

vec2 cellCenter(ivec2 cr) {
  float c = uCell;
  if (uLayout == 0) return (vec2(cr) + 0.5) * c;
  float i = float(cr.x - 1);
  float x = (i + 0.5 * float(cr.y & 1)) * c;
  float y = float(cr.y) * SQRT3_2 * c;
  return uLayout == 2 ? vec2(y, x) : vec2(x, y);
}

ivec2 lookupCell(vec2 p) {
  float c = uCell;
  ivec2 cr;
  if (uLayout == 0) {
    cr = ivec2(floor(p / c));
  } else {
    vec2 q = uLayout == 2 ? p.yx : p;
    float pitch = SQRT3_2 * c;
    int j0 = int(floor(q.y / pitch));
    float best = 1e30;
    cr = ivec2(0);
    for (int k = 0; k < 2; k++) {
      int j = j0 + k;
      float off = 0.5 * float(j & 1);
      float i = floor(q.x / c - off + 0.5);
      vec2 d = q - vec2((i + off) * c, float(j) * pitch);
      float d2 = dot(d, d);
      if (d2 < best) {
        best = d2;
        cr = ivec2(int(i) + 1, j);
      }
    }
  }
  return clamp(cr, ivec2(0), uGrid - 1);
}

float cellExit(ivec2 cr, vec2 p, vec2 d) {
  vec2 r = p - cellCenter(cr);
  float a = 0.5 * uCell;
  float t = 1e30;
  int n = uLayout == 0 ? 4 : 6;
  float stepA = 2.0 * PI / float(n);
  float phase = uLayout == 2 ? PI / 6.0 : 0.0;
  for (int k = 0; k < 6; k++) {
    if (k >= n) break;
    vec2 nn = vec2(cos(phase + float(k) * stepA), sin(phase + float(k) * stepA));
    float dn = dot(d, nn);
    if (dn > 1e-9) t = min(t, (a - dot(r, nn)) / dn);
  }
  return max(0.0, t);
}

float planeAt(ivec2 cr, vec2 p) {
  vec4 pl = texelFetch(uPlanes, cr, 0);
  if (pl.w < 0.5) return -1e30;
  vec2 c = cellCenter(cr);
  return pl.x + dot(pl.yz, p - c);
}

// 1.0 = lit, 0.0 = in a cast shadow.
float traceVisibility(vec3 P, vec3 toLight, float tMaxIn, bool onFacet) {
  float hl = length(toLight.xy);
  if (hl < 1e-6) return 1.0;
  vec2 d = toLight.xy / hl;
  float slope = toLight.z / hl;
  float tMax = tMaxIn;
  float t;
  if (onFacet) {
    ivec2 c0 = lookupCell(P.xy);
    t = cellExit(c0, P.xy, d) + TRACE_EPS;
  } else {
    t = WALL_START;
  }
  for (int s = 0; s < MAX_STEPS; s++) {
    if (t >= tMax) return 1.0;
    vec2 p = P.xy + d * t;
    if (p.x < 0.0 || p.y < 0.0 || p.x > uPlate.x || p.y > uPlate.y) return 1.0;
    float zr = P.z + slope * t;
    if (slope >= 0.0 && zr > uZMax + BIAS) return 1.0;
    ivec2 cr = lookupCell(p);
    float te = min(t + cellExit(cr, p, d), tMax);
    float s0 = planeAt(cr, p) - zr;
    float s1 = planeAt(cr, P.xy + d * te) - (P.z + slope * te);
    if (max(s0, s1) > BIAS) return 0.0;
    t = te + TRACE_EPS;
  }
  return 1.0;
}

void main() {
  vec3 n = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
  vec3 L;
  float irr = uDirect;
  float tMax = 1e30;
  if (uLightKind == 1) {
    vec3 toL = uLightPos - vPlate;
    float d2 = max(dot(toL, toL), 1e-6);
    L = toL * inversesqrt(d2);
    irr = uDirect * uRefDist2 / d2;
    tMax = length(toL.xy);
  } else {
    L = uLightDir;
  }
  float ndl = dot(n, L);
  float vis = 0.0;
  if (ndl > 0.0) {
    vis = 1.0;
    if (uHasPlanes && uShadowStrength > 0.0) {
      vec3 toLight = uLightKind == 1 ? (uLightPos - vPlate) : L;
      float v = traceVisibility(vPlate, toLight, tMax, n.z > 0.7);
      vis = mix(1.0, v, uShadowStrength);
    }
  }
  vec3 radiance = uAlbedo / PI * (irr * max(ndl, 0.0) * vis + uAmbient);
  gl_FragColor = vec4(radiance, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}
`;

export interface HeightfieldUniforms {
  [name: string]: THREE.IUniform;
  uPlanes: THREE.IUniform<THREE.DataTexture | null>;
  uGrid: THREE.IUniform<THREE.Vector2>;
  uLayout: THREE.IUniform<number>;
  uCell: THREE.IUniform<number>;
  uPlate: THREE.IUniform<THREE.Vector2>;
  uZMax: THREE.IUniform<number>;
  uHasPlanes: THREE.IUniform<boolean>;
  uLightKind: THREE.IUniform<number>;
  uLightDir: THREE.IUniform<THREE.Vector3>;
  uLightPos: THREE.IUniform<THREE.Vector3>;
  uRefDist2: THREE.IUniform<number>;
  uDirect: THREE.IUniform<number>;
  uAmbient: THREE.IUniform<number>;
  uAlbedo: THREE.IUniform<THREE.Color>;
  uShadowStrength: THREE.IUniform<number>;
}

export type HeightfieldMaterial = THREE.ShaderMaterial & { uniforms: HeightfieldUniforms };

export function createHeightfieldMaterial(albedo: THREE.Color): HeightfieldMaterial {
  const uniforms: HeightfieldUniforms = {
    uPlanes: { value: null },
    // ivec2 uniforms take a Vector2 in three.js.
    uGrid: { value: new THREE.Vector2(1, 1) },
    uLayout: { value: 0 },
    uCell: { value: 6 },
    uPlate: { value: new THREE.Vector2(1, 1) },
    uZMax: { value: 0 },
    uHasPlanes: { value: false },
    uLightKind: { value: 0 },
    uLightDir: { value: new THREE.Vector3(0, 0, 1) },
    uLightPos: { value: new THREE.Vector3(0, 0, 100) },
    uRefDist2: { value: 1 },
    uDirect: { value: 1 },
    uAmbient: { value: 0 },
    uAlbedo: { value: albedo.clone() },
    uShadowStrength: { value: 1 },
  };
  const material = new THREE.ShaderMaterial({ uniforms, vertexShader, fragmentShader, side: THREE.FrontSide });
  material.name = 'squama-heightfield';
  return material as HeightfieldMaterial;
}

/** Uploads cell planes into the material (reusing the texture when the grid size is unchanged). */
export function setPlanes(material: HeightfieldMaterial, planes: CellPlanes | null): void {
  const u = material.uniforms;
  if (!planes || planes.validCells === 0) {
    u.uHasPlanes.value = false;
    return;
  }
  let tex = u.uPlanes.value;
  if (!tex || tex.image.width !== planes.cols || tex.image.height !== planes.rows) {
    tex?.dispose();
    tex = new THREE.DataTexture(planes.data, planes.cols, planes.rows, THREE.RGBAFormat, THREE.FloatType);
    tex.magFilter = THREE.NearestFilter;
    tex.minFilter = THREE.NearestFilter;
    tex.generateMipmaps = false;
    tex.flipY = false;
    u.uPlanes.value = tex;
  } else {
    (tex.image.data as Float32Array).set(planes.data);
  }
  tex.needsUpdate = true;
  u.uGrid.value.set(planes.cols, planes.rows);
  u.uLayout.value = layoutCode(planes.layout);
  u.uCell.value = planes.layout.cellMm;
  u.uPlate.value.set(planes.layout.widthMm, planes.layout.heightMm);
  u.uZMax.value = planes.zMax;
  u.uHasPlanes.value = true;
}

/** The GLSL source, for tests that check it stays in step with heightfield.ts. */
export const HEIGHTFIELD_GLSL = { vertexShader, fragmentShader };
