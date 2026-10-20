// Browser stand-in for the Rust engine. Meshes are generated in a Web Worker (inline when Worker
// is unavailable), encoded in the same binary format as Rust and parsed by the same code, so the
// whole UI path runs in a plain browser. Spike output stays in memory and on `window`.

import { runMockJob, type MockJob } from './mockCore';
import type { MockWorkerRequest, MockWorkerResponse } from './mockWorker';
import { parseMeshPayload } from './payload';
import { assertArtifactName, type Engine, type MeshPayload } from './types';

interface Arrival {
  buffer: ArrayBuffer;
  receivedAt: number;
}

export interface MockEngineOptions {
  /** Defaults to true when the Worker API exists. */
  useWorker?: boolean;
}

export function createMockEngine(options: MockEngineOptions = {}): Engine {
  let workerAllowed = options.useWorker ?? typeof Worker !== 'undefined';
  let worker: Worker | null = null;
  let nextId = 1;
  const pending = new Map<number, { resolve: (a: Arrival) => void; reject: (e: Error) => void }>();
  const artifacts = new Map<string, Uint8Array>();

  function failAll(error: Error) {
    for (const p of pending.values()) p.reject(error);
    pending.clear();
  }

  function getWorker(): Worker | null {
    if (!workerAllowed) return null;
    if (worker) return worker;
    try {
      const w = new Worker(new URL('./mockWorker.ts', import.meta.url), { type: 'module' });
      w.onmessage = (event: MessageEvent<MockWorkerResponse>) => {
        const receivedAt = performance.now();
        const msg = event.data;
        const p = pending.get(msg.id);
        if (!p) return;
        pending.delete(msg.id);
        if (msg.ok) p.resolve({ buffer: msg.buffer, receivedAt });
        else p.reject(new Error(msg.error));
      };
      w.onerror = (event) => {
        failAll(new Error(`mock worker error: ${event.message || 'unknown'}`));
        w.terminate();
        worker = null;
      };
      worker = w;
    } catch (err) {
      console.warn('[mock] Worker unavailable, generating on the main thread', err);
      workerAllowed = false;
      worker = null;
    }
    return worker;
  }

  function run(job: MockJob): Promise<Arrival> {
    const w = getWorker();
    if (!w) {
      return new Promise((resolve, reject) => {
        setTimeout(() => {
          try {
            const buffer = runMockJob(job);
            resolve({ buffer, receivedAt: performance.now() });
          } catch (err) {
            reject(err instanceof Error ? err : new Error(String(err)));
          }
        }, 0);
      });
    }
    return new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      const request: MockWorkerRequest = { id, job };
      w.postMessage(request);
    });
  }

  async function fetchMesh(job: MockJob): Promise<MeshPayload> {
    const requestedAt = performance.now();
    const { buffer, receivedAt } = await run(job);
    const payload = parseMeshPayload(buffer);
    payload.timing = { requestedAt, receivedAt, parsedAt: performance.now() };
    return payload;
  }

  return {
    name: 'mock',
    isNative: false,
    generateSquama(params) {
      if (params.image !== 'synthetic') {
        console.warn(`[mock] cannot read ${params.image} in the browser; using the synthetic image`);
      }
      return fetchMesh({ kind: 'squama', params: { ...params } });
    },
    syntheticMesh(nTriangles) {
      return fetchMesh({ kind: 'synthetic', nTriangles });
    },
    async getLaunchFlags() {
      return { spike: false };
    },
    async saveArtifact(name, bytes) {
      assertArtifactName(name);
      artifacts.set(name, bytes);
      if (typeof window !== 'undefined') {
        window.__SPIKE_ARTIFACTS__ = Object.fromEntries(artifacts);
      }
      console.log(`[spike] artifact saved: ${name} (${bytes.byteLength} bytes)`);
    },
    async finishSpike(report) {
      if (typeof window !== 'undefined') window.__SPIKE_REPORT__ = report;
      console.log('[spike] report ' + JSON.stringify(report));
    },
  };
}
