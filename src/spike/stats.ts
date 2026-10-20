// Small statistics helpers for the spike report.

/** Linear-interpolated percentile (numpy's default); NaN for an empty list. */
export function percentile(values: readonly number[], p: number): number {
  const xs = values.filter((v) => Number.isFinite(v)).sort((a, b) => a - b);
  if (xs.length === 0) return Number.NaN;
  const rank = (Math.min(100, Math.max(0, p)) / 100) * (xs.length - 1);
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  return xs[lo] + (xs[hi] - xs[lo]) * (rank - lo);
}

export function median(values: readonly number[]): number {
  return percentile(values, 50);
}

/** Rounds for the report; non-finite values become null so the JSON stays valid. */
export function round(value: number, digits = 2): number | null {
  if (!Number.isFinite(value)) return null;
  const f = 10 ** digits;
  return Math.round(value * f) / f;
}

/** Counts timestamps per consecutive window of `windowMs`, starting at `start`. */
export function countPerWindow(times: readonly number[], start: number, windows: number, windowMs = 1000): number[] {
  const out = new Array<number>(windows).fill(0);
  for (const t of times) {
    const w = Math.floor((t - start) / windowMs);
    if (w >= 0 && w < windows) out[w]++;
  }
  return out;
}

/** Differences between consecutive values. */
export function intervals(times: readonly number[]): number[] {
  const out: number[] = [];
  for (let i = 1; i < times.length; i++) out.push(times[i] - times[i - 1]);
  return out;
}
