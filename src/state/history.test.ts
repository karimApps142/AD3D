import { describe, expect, it } from 'vitest';
import { History } from './history';

function clock(start = 0) {
  let t = start;
  return { now: () => t, advance: (ms: number) => (t += ms) };
}

describe('History', () => {
  it('records, undoes and redoes', () => {
    const h = new History(0);
    h.record(1, 'a');
    h.record(2, 'b');
    expect(h.present).toBe(2);
    expect(h.undo()).toBe(1);
    expect(h.undo()).toBe(0);
    expect(h.undo()).toBeNull();
    expect(h.canUndo).toBe(false);
    expect(h.redo()).toBe(1);
    expect(h.redo()).toBe(2);
    expect(h.redo()).toBeNull();
  });

  it('ignores no-op records', () => {
    const h = new History(5);
    h.record(5, 'a');
    expect(h.canUndo).toBe(false);
  });

  it('a new change clears redo', () => {
    const h = new History(0);
    h.record(1, 'a');
    h.undo();
    expect(h.canRedo).toBe(true);
    h.record(7, 'b');
    expect(h.canRedo).toBe(false);
    expect(h.undo()).toBe(0);
  });

  it('coalesces a whole gesture into one step, whatever it changes', () => {
    const c = clock();
    const h = new History(0, { now: c.now, coalesceMs: 100 });
    h.beginGesture();
    for (let v = 1; v <= 50; v++) {
      c.advance(500); // slow drag: far beyond the time window
      h.record(v, v % 2 ? 'x' : 'y');
    }
    h.endGesture();
    expect(h.undoDepth).toBe(1);
    expect(h.undo()).toBe(0);
    expect(h.redo()).toBe(50);
  });

  it('separate gestures are separate steps', () => {
    const h = new History(0);
    h.beginGesture();
    h.record(1, 'w');
    h.record(2, 'w');
    h.endGesture();
    h.beginGesture();
    h.record(3, 'w');
    h.endGesture();
    expect(h.undoDepth).toBe(2);
    expect(h.undo()).toBe(2);
  });

  it('a gesture that returns to its start leaves no step', () => {
    const h = new History(10);
    h.record(11, 'k'); // earlier, unrelated step
    h.beginGesture();
    h.record(12, 'w');
    h.record(13, 'w');
    h.record(11, 'w');
    h.endGesture();
    expect(h.undoDepth).toBe(1);
    expect(h.undo()).toBe(10);
  });

  it('coalesces same-key changes inside the time window (typing, arrow keys)', () => {
    const c = clock();
    const h = new History('', { now: c.now, coalesceMs: 1000 });
    for (const s of ['3', '30', '300']) {
      c.advance(200);
      h.record(s, 'widthMm');
    }
    expect(h.undoDepth).toBe(1);
    c.advance(1500);
    h.record('3000', 'widthMm');
    expect(h.undoDepth).toBe(2);
  });

  it('does not coalesce different keys or after undo', () => {
    const c = clock();
    const h = new History(0, { now: c.now });
    h.record(1, 'a');
    h.record(2, 'b');
    expect(h.undoDepth).toBe(2);
    h.undo();
    h.record(3, 'a');
    h.record(4, 'a');
    // 3 starts a new step after undo, 4 merges into it.
    expect(h.undoDepth).toBe(2);
    expect(h.undo()).toBe(1);
  });

  it('null keys never time-coalesce', () => {
    const h = new History(0);
    h.record(1);
    h.record(2);
    expect(h.undoDepth).toBe(2);
  });

  it('a change right after a gesture does not join it', () => {
    const c = clock();
    const h = new History(0, { now: c.now });
    h.beginGesture();
    h.record(1, 'w');
    h.endGesture();
    h.record(2, 'w');
    expect(h.undoDepth).toBe(2);
  });

  it('caps the depth, dropping the oldest steps', () => {
    const h = new History(0, { limit: 3 });
    for (let v = 1; v <= 10; v++) h.record(v);
    expect(h.undoDepth).toBe(3);
    h.undo();
    h.undo();
    expect(h.undo()).toBe(7);
    expect(h.canUndo).toBe(false);
    h.setLimit(1);
    expect(h.undoDepth).toBe(0);
  });

  it('reset forgets everything', () => {
    const h = new History(0);
    h.record(1);
    h.undo();
    h.reset(9);
    expect(h.present).toBe(9);
    expect(h.canUndo || h.canRedo).toBe(false);
  });

  it('nested gestures count', () => {
    const h = new History(0);
    h.beginGesture();
    h.beginGesture();
    h.record(1, 'a');
    h.endGesture();
    expect(h.inGesture).toBe(true);
    h.record(2, 'b');
    h.endGesture();
    h.endGesture(); // extra end is harmless
    expect(h.undoDepth).toBe(1);
  });

  it('randomised: undo all returns to the start, redo all returns to the end', () => {
    let seed = 12345;
    const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31);
    for (let trial = 0; trial < 200; trial++) {
      const c = clock();
      const h = new History(0, { now: c.now, limit: 10_000, coalesceMs: 50 });
      const visited: number[] = [0];
      for (let i = 0; i < 100; i++) {
        const r = rnd();
        c.advance(Math.floor(rnd() * 100));
        if (r < 0.1) h.beginGesture();
        else if (r < 0.2) h.endGesture();
        else if (r < 0.3) h.undo();
        else if (r < 0.35) h.redo();
        else h.record(Math.floor(rnd() * 5), rnd() < 0.5 ? 'a' : 'b');
        visited.push(h.present);
      }
      const end = h.present;
      while (h.canRedo) h.redo();
      const tip = h.present;
      while (h.canUndo) h.undo();
      expect(h.present).toBe(0);
      while (h.canRedo) h.redo();
      expect(h.present).toBe(tip);
      expect(visited).toContain(end);
    }
  });
});
