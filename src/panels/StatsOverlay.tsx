// Live numbers in the viewport corner. Reads the mutable perf counters 4× per second, so the hot
// paths never touch React.

import { useEffect, useState } from 'react';
import { perf, ratePerSecond } from '../state/perf';
import { useAppStore } from '../state/store';

function ms(v: number): string {
  return Number.isFinite(v) ? `${v < 10 ? v.toFixed(2) : v.toFixed(1)} ms` : '–';
}

function count(v: number): string {
  return v.toLocaleString('en-US');
}

function mb(bytes: number): string {
  return bytes > 0 ? `${(bytes / 1048576).toFixed(2)} MB` : '–';
}

interface Snapshot {
  fps: number;
  ups: number;
  last: typeof perf.last;
}

function snapshot(): Snapshot {
  const now = performance.now();
  return { fps: ratePerSecond(perf.frames, now), ups: ratePerSecond(perf.updates, now), last: { ...perf.last } };
}

export function StatsOverlay() {
  const [s, setS] = useState<Snapshot>(snapshot);
  const engine = useAppStore((st) => st.engine);
  useEffect(() => {
    const id = window.setInterval(() => setS(snapshot()), 250);
    return () => window.clearInterval(id);
  }, []);

  const rows: Array<[string, string]> = [
    ['render fps', String(s.fps)],
    ['preview updates/s', String(s.ups)],
    [engine?.isNative ? 'Rust gen' : 'mock gen', ms(s.last.genMs)],
    ['encode', ms(s.last.encodeMs)],
    ['IPC / receive', ms(s.last.ipcMs)],
    ['parse', ms(s.last.parseMs)],
    ['geometry build', `${ms(s.last.geometryMs)}${s.last.geometryMode ? ` (${s.last.geometryMode})` : ''}`],
    ['render CPU', ms(s.last.renderMs)],
    ['input → frame', ms(s.last.latencyMs)],
    ['triangles', count(s.last.triangles)],
    ['vertices', count(s.last.vertices)],
    ['payload', mb(s.last.bytes)],
  ];

  return (
    <div className="stats" aria-label="Performance">
      <div className="stats-title">
        {engine ? (engine.isNative ? 'Rust engine' : 'Mock engine (browser)') : 'Starting…'}
      </div>
      <table>
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <th scope="row">{k}</th>
              <td>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
