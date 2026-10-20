// Latest-wins request loop for live previews.
//
// - At most one request is in flight.
// - While one is in flight, only the newest pending params are kept (older ones are coalesced).
// - When the in-flight request returns, the pending params (if any) are sent at once.
// - A response is applied only if it is newer than the last applied one and was not invalidated;
//   it must also echo its job id when `getJobId` is given. Everything else is dropped as stale.

export interface LatestWinsStats {
  /** request() calls. */
  requested: number;
  /** Requests actually sent. */
  sent: number;
  /** Pending params replaced by newer ones before being sent. */
  coalesced: number;
  /** Responses handed to onResult. */
  applied: number;
  /** Responses dropped as stale (old job id, invalidated, or wrong echo). */
  dropped: number;
  errors: number;
}

export interface LatestWinsOptions<P, R> {
  run: (params: P, jobId: number) => Promise<R>;
  onResult: (result: R, info: { jobId: number; params: P; requestedAt: number; sentAt: number }) => void;
  onError?: (error: unknown, info: { jobId: number; params: P }) => void;
  /** Job id echoed by the result; mismatches are dropped. */
  getJobId?: (result: R) => number;
  /** Called whenever the busy state flips. */
  onBusyChange?: (busy: boolean) => void;
}

interface Slot<P> {
  params: P;
  /** performance.now() of the request() call that produced these params. */
  requestedAt: number;
}

export class LatestWinsLoop<P, R> {
  readonly stats: LatestWinsStats = {
    requested: 0,
    sent: 0,
    coalesced: 0,
    applied: 0,
    dropped: 0,
    errors: 0,
  };

  private readonly opts: LatestWinsOptions<P, R>;
  private nextJobId = 1;
  private inFlight: (Slot<P> & { jobId: number }) | null = null;
  private pending: Slot<P> | null = null;
  private minValidJobId = 0;
  private lastAppliedJobId = 0;
  private paused = false;
  private busyReported = false;
  private idleWaiters: Array<() => void> = [];

  constructor(options: LatestWinsOptions<P, R>) {
    this.opts = options;
  }

  get busy(): boolean {
    return this.inFlight !== null;
  }

  get hasPending(): boolean {
    return this.pending !== null;
  }

  /** Job id the next send will use. */
  get peekNextJobId(): number {
    return this.nextJobId;
  }

  request(params: P, now: number = performance.now()): void {
    this.stats.requested++;
    if (this.inFlight !== null || this.paused) {
      if (this.pending !== null) this.stats.coalesced++;
      this.pending = { params, requestedAt: now };
      return;
    }
    this.send({ params, requestedAt: now });
  }

  /** Drops every response to requests sent so far, and any pending params. */
  invalidate(): void {
    this.minValidJobId = this.nextJobId;
    this.pending = null;
  }

  /** Stops sending; new params queue up (newest only) until resume(). */
  pause(): void {
    this.paused = true;
  }

  resume(): void {
    this.paused = false;
    if (this.inFlight === null && this.pending !== null) {
      const next = this.pending;
      this.pending = null;
      this.send(next);
    }
  }

  /** Resolves once nothing is in flight (pending params held by pause() do not count). */
  whenIdle(): Promise<void> {
    if (this.inFlight === null && (this.pending === null || this.paused)) return Promise.resolve();
    return new Promise((resolve) => this.idleWaiters.push(resolve));
  }

  private send(slot: Slot<P>): void {
    const jobId = this.nextJobId;
    if (jobId >= 0xffffffff) {
      // Job ids are u32 on the wire. Wrapping takes ~2 years of continuous dragging; start over.
      this.nextJobId = 1;
      this.lastAppliedJobId = 0;
      this.minValidJobId = 0;
    } else {
      this.nextJobId = jobId + 1;
    }
    const sentAt = performance.now();
    this.inFlight = { ...slot, jobId };
    this.stats.sent++;
    if (!this.busyReported) {
      this.busyReported = true;
      this.opts.onBusyChange?.(true);
    }
    let promise: Promise<R>;
    try {
      promise = this.opts.run(slot.params, jobId);
    } catch (err) {
      promise = Promise.reject(err);
    }
    promise.then(
      (result) => this.finish(jobId, slot, sentAt, result, undefined, false),
      (error) => this.finish(jobId, slot, sentAt, undefined, error, true),
    );
  }

  private finish(
    jobId: number,
    slot: Slot<P>,
    sentAt: number,
    result: R | undefined,
    error: unknown,
    failed: boolean,
  ): void {
    this.inFlight = null;
    if (failed) {
      this.stats.errors++;
      this.opts.onError?.(error, { jobId, params: slot.params });
    } else {
      const echoed = this.opts.getJobId ? this.opts.getJobId(result as R) : jobId;
      if (jobId < this.minValidJobId || jobId <= this.lastAppliedJobId || echoed !== jobId) {
        this.stats.dropped++;
      } else {
        this.lastAppliedJobId = jobId;
        this.stats.applied++;
        try {
          this.opts.onResult(result as R, { jobId, params: slot.params, requestedAt: slot.requestedAt, sentAt });
        } catch (err) {
          this.stats.errors++;
          this.opts.onError?.(err, { jobId, params: slot.params });
        }
      }
    }
    if (this.pending !== null && !this.paused) {
      const next = this.pending;
      this.pending = null;
      this.send(next);
      return;
    }
    this.busyReported = false;
    this.opts.onBusyChange?.(false);
    const waiters = this.idleWaiters;
    this.idleWaiters = [];
    for (const w of waiters) w();
  }
}
