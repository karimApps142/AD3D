// Simulates a user dragging a real <input type="range">: it sets the value through the native
// setter (so React's value tracker sees a change) and dispatches 'input'.
//
//   raf    one value per animation frame. Updates/s can never exceed the rAF rate, and input never
//          arrives faster than frames, so latest-wins coalescing is not exercised.
//   timer  one value every `intervalMs` from a timer, independent of rAF (default 4 ms ≈ 250 Hz,
//          faster than any display), like a high-rate trackpad: several inputs land between frames,
//          so the preview loop has to coalesce and drop work.

export function setNativeValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  if (!setter) throw new Error('HTMLInputElement value setter not found');
  setter.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

/** Triangle wave between lo and hi with the given period, starting at lo. */
export function triangleWave(t: number, lo: number, hi: number, periodMs: number): number {
  const phase = (((t / periodMs) % 1) + 1) % 1;
  const u = phase < 0.5 ? phase * 2 : 2 - phase * 2;
  return lo + (hi - lo) * u;
}

export interface DragOptions {
  durationMs: number;
  lo: number;
  hi: number;
  periodMs: number;
  /** Decimal places of the slider's step. */
  digits: number;
  /** How input is paced (default 'raf'). */
  driver?: 'raf' | 'timer';
  /** Input interval for the 'timer' driver (ms, default 4). */
  intervalMs?: number;
}

export interface DragResult {
  startedAt: number;
  endedAt: number;
  /** 'input' events dispatched. */
  inputEvents: number;
  /** Events that actually changed the slider's value. */
  valueChanges: number;
}

function pointer(type: string): Event {
  return typeof PointerEvent === 'function'
    ? new PointerEvent(type, { bubbles: true, pointerId: 1, isPrimary: true })
    : new MouseEvent(type.replace('pointer', 'mouse'), { bubbles: true });
}

export function dragSlider(input: HTMLInputElement, opts: DragOptions): Promise<DragResult> {
  return new Promise((resolve) => {
    let inputEvents = 0;
    let valueChanges = 0;
    const startedAt = performance.now();
    input.dispatchEvent(pointer('pointerdown'));
    let timer: ReturnType<typeof setInterval> | null = null;
    let done = false;
    const tick = (): boolean => {
      if (done) return false;
      const now = performance.now();
      const t = now - startedAt;
      if (t >= opts.durationMs) {
        done = true;
        if (timer !== null) clearInterval(timer);
        input.dispatchEvent(pointer('pointerup'));
        input.dispatchEvent(new Event('change', { bubbles: true }));
        resolve({ startedAt, endedAt: now, inputEvents, valueChanges });
        return false;
      }
      const before = input.value;
      setNativeValue(input, triangleWave(t, opts.lo, opts.hi, opts.periodMs).toFixed(opts.digits));
      inputEvents++;
      if (input.value !== before) valueChanges++;
      return true;
    };
    if (opts.driver === 'timer') {
      timer = setInterval(tick, Math.max(1, opts.intervalMs ?? 4));
    } else {
      const frame = () => {
        if (tick()) requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    }
  });
}
