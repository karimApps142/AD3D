// The latest core report about the plate on screen (layout + warnings), published by the engines
// after each generation without delaying the mesh itself.

import { create } from 'zustand';
import type { SquamaJobInfo } from './types';

export interface SquamaInfoState {
  info: SquamaJobInfo | null;
  publish: (info: SquamaJobInfo) => void;
}

export const useSquamaInfo = create<SquamaInfoState>()((set, get) => ({
  info: null,
  publish: (info) => {
    const cur = get().info;
    // Job ids only grow (latest-wins loop); never let an older report replace a newer one.
    if (cur && info.jobId < cur.jobId && cur.jobId - info.jobId < 0x7fffffff) return;
    set({ info });
  },
}));
