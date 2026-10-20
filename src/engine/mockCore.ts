// One mock job: generate, then encode into the binary payload, timing both like the Rust side.
// Runs inside the mock worker (or inline when Worker is unavailable, e.g. in unit tests).

import { buildMockSquama, buildSyntheticMesh, type RawMesh } from './mockMesh';
import { encodeMeshPayload, setPayloadTimes } from './payload';
import type { SquamaParamsDto } from './types';

export type MockJob =
  | { kind: 'squama'; params: SquamaParamsDto }
  | { kind: 'synthetic'; nTriangles: number };

export function runMockJob(job: MockJob): ArrayBuffer {
  const t0 = performance.now();
  let mesh: RawMesh;
  let jobId = 0;
  if (job.kind === 'squama') {
    mesh = buildMockSquama(job.params);
    jobId = job.params.jobId;
  } else {
    mesh = buildSyntheticMesh(job.nTriangles);
  }
  const t1 = performance.now();
  const buffer = encodeMeshPayload({
    positions: mesh.positions,
    indices: mesh.indices,
    vertexCount: mesh.vertexCount,
    indexCount: mesh.indexCount,
    bboxMin: mesh.bboxMin,
    bboxMax: mesh.bboxMax,
    jobId,
  });
  const t2 = performance.now();
  setPayloadTimes(buffer, t1 - t0, t2 - t1);
  return buffer;
}
