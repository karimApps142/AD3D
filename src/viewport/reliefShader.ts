// Relief colour map: facets coloured by height (turbo map over the facet height range), walls,
// back, keys and stand in fixed colours, with a soft headlight so the shape still reads. Face kinds
// come from the payload (§12.2 flags bit 1, de-indexed geometry with a per-vertex `aKind`) or, when
// the payload has none, from the flat triangle normal.

import * as THREE from 'three';
import { FACE_KIND, type FaceKind } from './faceKinds';

/** Linear-ish display colours per kind (facets use the height map instead). */
export const KIND_COLORS: Record<Exclude<FaceKind, 0>, string> = {
  [FACE_KIND.wall]: '#8d939c',
  [FACE_KIND.back]: '#3a4150',
  [FACE_KIND.key]: '#ff9b3d',
  [FACE_KIND.stand]: '#3fc1b0',
};

/** Polynomial fit of Google's Turbo colour map (Mikhailov 2019), t ∈ [0, 1] → sRGB. */
export function turbo(t: number): [number, number, number] {
  const x = Math.min(1, Math.max(0, t));
  const x2 = x * x, x3 = x2 * x, x4 = x3 * x, x5 = x4 * x;
  const r = 0.13572138 + 4.6153926 * x - 42.66032258 * x2 + 132.13108234 * x3 - 152.94239396 * x4 + 59.28637943 * x5;
  const g = 0.09140261 + 2.19418839 * x + 4.84296658 * x2 - 14.18503333 * x3 + 4.27729857 * x4 + 2.82956604 * x5;
  const b = 0.1066733 + 12.64194608 * x - 60.58204836 * x2 + 110.36276771 * x3 - 89.90310912 * x4 + 27.34824973 * x5;
  const c = (v: number) => Math.min(1, Math.max(0, v));
  return [c(r), c(g), c(b)];
}

const vertexShader = /* glsl */ `
#ifdef HAS_KINDS
attribute float aKind;
attribute float aPart;
varying float vKind;
varying float vPart;
#endif
varying vec3 vWorld;
void main() {
#ifdef HAS_KINDS
  vKind = aKind;
  vPart = aPart;
#endif
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  gl_Position = projectionMatrix * viewMatrix * world;
}
`;

const fragmentShader = /* glsl */ `
#ifdef HAS_KINDS
varying float vKind;
varying float vPart;
#endif
varying vec3 vWorld;
uniform vec2 uRange;          // facet height range (w min, w max)
uniform vec3 uWall;
uniform vec3 uBack;
uniform vec3 uKey;
uniform vec3 uStand;

vec3 turbo(float x) {
  x = clamp(x, 0.0, 1.0);
  vec4 v4 = vec4(1.0, x, x * x, x * x * x);
  vec2 v2 = v4.zw * v4.z;
  return clamp(vec3(
    dot(v4, vec4(0.13572138, 4.6153926, -42.66032258, 132.13108234)) + dot(v2, vec2(-152.94239396, 59.28637943)),
    dot(v4, vec4(0.09140261, 2.19418839, 4.84296658, -14.18503333)) + dot(v2, vec2(4.27729857, 2.82956604)),
    dot(v4, vec4(0.1066733, 12.64194608, -60.58204836, 110.36276771)) + dot(v2, vec2(-89.90310912, 27.34824973))
  ), 0.0, 1.0);
}

void main() {
  vec3 n = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
#ifdef HAS_KINDS
  int kind = int(vKind + 0.5);
  float part = vPart;
#else
  int kind = n.z > 0.5 ? 0 : (n.z < -0.5 ? 2 : 1);
  float part = 0.0;
#endif
  vec3 srgb;
  if (kind == 0) {
    float t = (vWorld.z - uRange.x) / max(uRange.y - uRange.x, 1e-6);
    srgb = turbo(t);
  } else if (kind == 1) srgb = uWall;
  else if (kind == 2) srgb = uBack;
  else if (kind == 3) srgb = uKey;
  else srgb = uStand;
  // Alternate parts (tiles) slightly darker so seams read.
  srgb *= mod(part, 2.0) > 0.5 ? 0.86 : 1.0;
  vec3 v = normalize(cameraPosition - vWorld);
  float shade = 0.45 + 0.55 * abs(dot(n, v));
  // The map is defined in sRGB; convert to linear so the output transform round-trips it.
  vec3 lin = pow(srgb, vec3(2.2)) * shade;
  gl_FragColor = vec4(lin, 1.0);
  #include <colorspace_fragment>
}
`;

function srgbVec(hex: string): THREE.Vector3 {
  const c = new THREE.Color(hex).convertLinearToSRGB();
  return new THREE.Vector3(c.r, c.g, c.b);
}

export type ReliefMaterial = THREE.ShaderMaterial & { uniforms: { uRange: THREE.IUniform<THREE.Vector2> } };

export function createReliefMaterial(withKinds: boolean): ReliefMaterial {
  const material = new THREE.ShaderMaterial({
    uniforms: {
      uRange: { value: new THREE.Vector2(0, 1) },
      uWall: { value: srgbVec(KIND_COLORS[FACE_KIND.wall]) },
      uBack: { value: srgbVec(KIND_COLORS[FACE_KIND.back]) },
      uKey: { value: srgbVec(KIND_COLORS[FACE_KIND.key]) },
      uStand: { value: srgbVec(KIND_COLORS[FACE_KIND.stand]) },
    },
    defines: withKinds ? { HAS_KINDS: '' } : {},
    vertexShader,
    fragmentShader,
  });
  material.name = withKinds ? 'squama-relief-kinds' : 'squama-relief';
  return material as ReliefMaterial;
}

/**
 * De-indexes a mesh so every triangle carries its own face kind and part (three.js has no
 * per-primitive attributes). Returns positions (9 floats per triangle) and per-vertex kind/part.
 */
export function deindexWithKinds(
  positions: ArrayLike<number>,
  indices: ArrayLike<number>,
  indexCount: number,
  kinds: ArrayLike<number>,
): { position: Float32Array<ArrayBuffer>; kind: Float32Array<ArrayBuffer>; part: Float32Array<ArrayBuffer> } {
  const nTri = Math.floor(indexCount / 3);
  const position = new Float32Array(nTri * 9);
  const kind = new Float32Array(nTri * 3);
  const part = new Float32Array(nTri * 3);
  for (let t = 0; t < nTri; t++) {
    const k = kinds[t] & 0xff;
    const p = (kinds[t] >> 8) & 0xff;
    for (let s = 0; s < 3; s++) {
      const src = indices[t * 3 + s] * 3;
      const dst = (t * 3 + s) * 3;
      position[dst] = positions[src];
      position[dst + 1] = positions[src + 1];
      position[dst + 2] = positions[src + 2];
      kind[t * 3 + s] = k;
      part[t * 3 + s] = p;
    }
  }
  return { position, kind, part };
}
