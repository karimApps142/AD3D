// Undo/redo history of immutable snapshots, with coalescing so that one slider drag (dozens of
// intermediate values) becomes one undo step.
//
// Coalescing rules, in order:
// 1. During a gesture (pointer down on a slider … pointer up), every change merges into the
//    entry the gesture opened, whatever it changed.
// 2. Outside gestures, a change merges into the previous entry when it has the same key (the
//    set of fields it changed) and arrives within `coalesceMs` of the previous change (typing a
//    number, holding an arrow key).
// 3. Ending a gesture, undo, redo and reset close the open entry, so the next change starts a
//    new step.
// A merge that lands back on the entry's starting state removes the entry (net no-op).

export interface HistoryOptions<T> {
  /** Maximum undo steps kept (oldest dropped first). */
  limit?: number;
  /** Time window for rule 2 (ms). */
  coalesceMs?: number;
  now?: () => number;
  equals?: (a: T, b: T) => boolean;
}

export class History<T> {
  private past: T[] = [];
  private future: T[] = [];
  private current: T;
  private limit: number;
  private readonly coalesceMs: number;
  private readonly now: () => number;
  private readonly equals: (a: T, b: T) => boolean;

  /** The open entry may still absorb changes. */
  private open = false;
  private openKey: string | null = null;
  private openAt = 0;
  private openedByGesture = false;
  private gestureDepth = 0;

  constructor(initial: T, opts: HistoryOptions<T> = {}) {
    this.current = initial;
    this.limit = Math.max(1, opts.limit ?? 200);
    this.coalesceMs = opts.coalesceMs ?? 1000;
    this.now = opts.now ?? (() => Date.now());
    this.equals = opts.equals ?? Object.is;
  }

  get present(): T {
    return this.current;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  get undoDepth(): number {
    return this.past.length;
  }

  get redoDepth(): number {
    return this.future.length;
  }

  get inGesture(): boolean {
    return this.gestureDepth > 0;
  }

  setLimit(limit: number): void {
    this.limit = Math.max(1, Math.floor(limit));
    this.trim();
  }

  /** Starts a gesture (rule 1). Nested calls are counted. */
  beginGesture(): void {
    if (this.gestureDepth === 0) this.close();
    this.gestureDepth++;
  }

  endGesture(): void {
    if (this.gestureDepth === 0) return;
    this.gestureDepth--;
    if (this.gestureDepth === 0) this.close();
  }

  /** Records that the state changed to `next`. `key` identifies what changed (rule 2). */
  record(next: T, key: string | null = null): void {
    if (this.equals(next, this.current)) return;
    const t = this.now();
    const merge =
      this.open &&
      this.past.length > 0 &&
      (this.gestureDepth > 0
        ? this.openedByGesture
        : !this.openedByGesture && key !== null && key === this.openKey && t - this.openAt <= this.coalesceMs);

    if (merge) {
      this.current = next;
      this.openAt = t;
      if (this.equals(this.past[this.past.length - 1], next)) {
        this.past.pop();
        this.open = false;
      }
    } else {
      this.past.push(this.current);
      this.current = next;
      this.open = true;
      this.openKey = key;
      this.openAt = t;
      this.openedByGesture = this.gestureDepth > 0;
      this.trim();
    }
    this.future = [];
  }

  undo(): T | null {
    const prev = this.past.pop();
    if (prev === undefined) return null;
    this.future.push(this.current);
    this.current = prev;
    this.close();
    return prev;
  }

  redo(): T | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(this.current);
    this.current = next;
    this.close();
    return next;
  }

  /** Forgets all steps (new or opened project). */
  reset(present: T): void {
    this.past = [];
    this.future = [];
    this.current = present;
    this.close();
  }

  private close(): void {
    this.open = false;
    this.openKey = null;
  }

  private trim(): void {
    if (this.past.length > this.limit) this.past.splice(0, this.past.length - this.limit);
  }
}
