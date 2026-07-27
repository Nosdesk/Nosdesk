// Host-side governor for the plugin bridge.
//
// Bounds a buggy or hostile plugin's ability to degrade shared capacity through
// the bridge: a call-rate limit, a max in-flight cap, and a per-call timeout.
// Every host API method a plugin invokes goes through `BridgeGovernor.run`, which
// rejects over-limit calls (the plugin sees a thrown error) rather than letting
// them pile up on the host. Per plugin instance — each frame gets its own budget.

export interface GovernorOptions {
  /** Max calls allowed within `windowMs`. */
  maxCallsPerWindow: number;
  windowMs: number;
  /** Max concurrently in-flight host calls. */
  maxInFlight: number;
  /** A single call must settle within this; else it rejects (the underlying work
   * may keep running host-side, but the plugin's call + its slot are freed). */
  callTimeoutMs: number;
}

export const DEFAULT_GOVERNOR_OPTIONS: GovernorOptions = {
  maxCallsPerWindow: 60,
  windowMs: 1000,
  maxInFlight: 8,
  callTimeoutMs: 15_000,
};

export class BridgeGovernor {
  private calls: number[] = [];
  private inFlight = 0;

  constructor(private readonly opts: GovernorOptions = DEFAULT_GOVERNOR_OPTIONS) {}

  async run<T>(fn: () => Promise<T>): Promise<T> {
    const now = Date.now();
    const windowStart = now - this.opts.windowMs;
    this.calls = this.calls.filter((t) => t > windowStart);
    if (this.calls.length >= this.opts.maxCallsPerWindow) {
      throw new Error(
        `plugin bridge rate limit exceeded (${this.opts.maxCallsPerWindow}/${this.opts.windowMs}ms)`,
      );
    }
    if (this.inFlight >= this.opts.maxInFlight) {
      throw new Error(`plugin bridge in-flight limit exceeded (${this.opts.maxInFlight})`);
    }
    this.calls.push(now);
    this.inFlight += 1;
    try {
      return await this.withTimeout(fn());
    } finally {
      this.inFlight -= 1;
    }
  }

  private withTimeout<T>(p: Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`plugin bridge call timed out after ${this.opts.callTimeoutMs}ms`)),
        this.opts.callTimeoutMs,
      );
      p.then(
        (v) => {
          clearTimeout(timer);
          resolve(v);
        },
        (e) => {
          clearTimeout(timer);
          reject(e as Error);
        },
      );
    });
  }
}

/**
 * Wrap a host API object so every method call is metered by `governor`. Recurses
 * into sub-objects (e.g. `tickets`, `assets`) so nested methods are covered too;
 * a method's *return value* is not re-wrapped, so a Comlink-proxied sub-API
 * returned from a method (e.g. `collections(name)`) keeps its own transport (its
 * per-call metering is a follow-up). Non-function properties pass through.
 */
export function governHostApi<T extends object>(impl: T, governor: BridgeGovernor): T {
  return new Proxy(impl, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver);
      if (typeof value === 'function') {
        return (...args: unknown[]) =>
          governor.run(() => Promise.resolve((value as (...a: unknown[]) => unknown).apply(target, args)));
      }
      if (value && typeof value === 'object') {
        return governHostApi(value as object, governor);
      }
      return value;
    },
  });
}
