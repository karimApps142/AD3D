import { describe, expect, it } from 'vitest';
import { LatestWinsLoop } from './latestWins';

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

function harness() {
  const calls: Array<{ params: number; jobId: number; d: Deferred<{ jobId: number; value: number }> }> = [];
  const applied: Array<{ value: number; jobId: number }> = [];
  const errors: unknown[] = [];
  const busy: boolean[] = [];
  const loop = new LatestWinsLoop<number, { jobId: number; value: number }>({
    run: (params, jobId) => {
      const d = deferred<{ jobId: number; value: number }>();
      calls.push({ params, jobId, d });
      return d.promise;
    },
    getJobId: (r) => r.jobId,
    onResult: (r, info) => applied.push({ value: r.value, jobId: info.jobId }),
    onError: (e) => errors.push(e),
    onBusyChange: (b) => busy.push(b),
  });
  const answer = (i: number) => calls[i].d.resolve({ jobId: calls[i].jobId, value: calls[i].params });
  return { loop, calls, applied, errors, busy, answer };
}

describe('latest-wins loop', () => {
  it('keeps at most one request in flight and only the newest pending params', async () => {
    const h = harness();
    h.loop.request(1);
    h.loop.request(2);
    h.loop.request(3);
    h.loop.request(4);
    expect(h.calls.map((c) => c.params)).toEqual([1]);
    expect(h.loop.stats.coalesced).toBe(2);
    h.answer(0);
    await flush();
    expect(h.applied.map((a) => a.value)).toEqual([1]);
    expect(h.calls.map((c) => c.params)).toEqual([1, 4]);
    h.answer(1);
    await flush();
    expect(h.applied.map((a) => a.value)).toEqual([1, 4]);
    expect(h.loop.busy).toBe(false);
    expect(h.loop.stats).toMatchObject({ requested: 4, sent: 2, applied: 2, dropped: 0 });
    expect(h.busy).toEqual([true, false]);
  });

  it('uses increasing job ids and sends them with the params', async () => {
    const h = harness();
    h.loop.request(10);
    h.answer(0);
    await flush();
    h.loop.request(11);
    expect(h.calls.map((c) => c.jobId)).toEqual([1, 2]);
  });

  it('drops responses whose echoed job id does not match', async () => {
    const h = harness();
    h.loop.request(1);
    h.calls[0].d.resolve({ jobId: 999, value: 1 });
    await flush();
    expect(h.applied).toEqual([]);
    expect(h.loop.stats.dropped).toBe(1);
  });

  it('drops in-flight responses after invalidate()', async () => {
    const h = harness();
    h.loop.request(1);
    h.loop.request(2);
    h.loop.invalidate();
    h.answer(0);
    await flush();
    expect(h.applied).toEqual([]);
    expect(h.loop.stats.dropped).toBe(1);
    expect(h.calls.length).toBe(1);
    h.loop.request(3);
    h.answer(1);
    await flush();
    expect(h.applied.map((a) => a.value)).toEqual([3]);
  });

  it('keeps going after an error', async () => {
    const h = harness();
    h.loop.request(1);
    h.loop.request(2);
    h.calls[0].d.reject(new Error('boom'));
    await flush();
    expect(h.errors.length).toBe(1);
    expect(h.calls.map((c) => c.params)).toEqual([1, 2]);
    h.answer(1);
    await flush();
    expect(h.applied.map((a) => a.value)).toEqual([2]);
  });

  it('pauses and resumes, and reports idle', async () => {
    const h = harness();
    h.loop.request(1);
    h.loop.pause();
    h.loop.request(2);
    let idle = false;
    void h.loop.whenIdle().then(() => {
      idle = true;
    });
    h.answer(0);
    await flush();
    expect(idle).toBe(true);
    expect(h.calls.length).toBe(1);
    expect(h.loop.hasPending).toBe(true);
    h.loop.resume();
    expect(h.calls.map((c) => c.params)).toEqual([1, 2]);
  });

  it('treats a throwing run() like a rejected one', async () => {
    const errors: unknown[] = [];
    const loop = new LatestWinsLoop<number, number>({
      run: () => {
        throw new Error('sync');
      },
      onResult: () => {},
      onError: (e) => errors.push(e),
    });
    loop.request(1);
    await flush();
    expect(errors.length).toBe(1);
    expect(loop.busy).toBe(false);
  });
});

describe('LatestWinsLoop cancel', () => {
  it('aborts a slow final job when a drag request arrives, and drops its outcome', async () => {
    const calls: Array<{ params: { q: string; v: number }; jobId: number; d: Deferred<{ jobId: number }> }> = [];
    const errors: unknown[] = [];
    const applied: number[] = [];
    let cancels = 0;
    const loop = new LatestWinsLoop<{ q: string; v: number }, { jobId: number }>({
      run: (params, jobId) => {
        const d = deferred<{ jobId: number }>();
        calls.push({ params, jobId, d });
        return d.promise;
      },
      getJobId: (r) => r.jobId,
      onResult: (_r, info) => applied.push(info.params.v),
      onError: (e) => errors.push(e),
      shouldCancel: (cur, next) => cur.q === 'final' && next.q === 'preview',
      cancel: () => {
        cancels++;
      },
    });
    loop.request({ q: 'final', v: 1 });
    expect(calls).toHaveLength(1);
    loop.request({ q: 'preview', v: 2 });
    loop.request({ q: 'preview', v: 3 });
    expect(cancels).toBe(1); // once per in-flight job
    // The core answers the cancelled job with an error: not shown, and the drag goes out at once.
    calls[0].d.reject(new Error('cancelled'));
    await flush();
    expect(errors).toEqual([]);
    expect(calls).toHaveLength(2);
    expect(calls[1].params.v).toBe(3);
    calls[1].d.resolve({ jobId: calls[1].jobId });
    await flush();
    expect(applied).toEqual([3]);
    // A preview in flight is never cancelled.
    loop.request({ q: 'preview', v: 4 });
    loop.request({ q: 'preview', v: 5 });
    expect(cancels).toBe(1);
  });
});
