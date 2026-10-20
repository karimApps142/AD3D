// Web Worker for the mock engine: keeps mesh generation off the UI thread and hands the encoded
// payload back as a transferable ArrayBuffer (no copy), mirroring the native IPC path.

import { runMockJob, type MockJob } from './mockCore';

export interface MockWorkerRequest {
  id: number;
  job: MockJob;
}

export type MockWorkerResponse =
  | { id: number; ok: true; buffer: ArrayBuffer }
  | { id: number; ok: false; error: string };

// The app tsconfig uses the DOM lib, so describe the worker scope we need by hand.
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<MockWorkerRequest>) => void) | null;
  postMessage(message: MockWorkerResponse, transfer: Transferable[]): void;
};

scope.onmessage = (event) => {
  const { id, job } = event.data;
  try {
    const buffer = runMockJob(job);
    scope.postMessage({ id, ok: true, buffer }, [buffer]);
  } catch (err) {
    scope.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) }, []);
  }
};
