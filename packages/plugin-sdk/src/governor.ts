// Host-side governor for the plugin bridge.
//
// Bounds a buggy or hostile plugin's ability to degrade shared capacity through
// the bridge: a call-rate limit, a max in-flight cap, and a per-call timeout.
// Every host API method a plugin invokes goes through `BridgeGovernor.run`, which
// rejects over-limit calls (the plugin sees a thrown error) rather than letting
// them pile up on the host. Per plugin instance — each frame gets its own budget.
//
// Interactive calls (a host dialog the user answers: `ui.confirm`, the pickers)
// take a second lane, `runInteractive`. They wait on a person, so they get no
// timeout and hold no in-flight slot, but they still count against the rate
// window, and only `maxInteractivePending` (one) may be open at a time.
import { PluginApiError } from './errors';

export interface GovernorOptions {
  /** Max calls allowed within `windowMs`. */
  maxCallsPerWindow: number;
  windowMs: number;
  /** Max concurrently in-flight host calls. */
  maxInFlight: number;
  /** A single call must settle within this; else it rejects (the underlying work
   * may keep running host-side, but the plugin's call + its slot are freed). */
  callTimeoutMs: number;
  /** Max interactive calls (host dialogs) pending at once. Optional so older
   * callers' option objects stay valid; defaults to 1. */
  maxInteractivePending?: number;
}

export const DEFAULT_GOVERNOR_OPTIONS: GovernorOptions = {
  maxCallsPerWindow: 60,
  windowMs: 1000,
  maxInFlight: 8,
  callTimeoutMs: 15_000,
  maxInteractivePending: 1,
};

/**
 * Host API method names that open a dialog the user answers, so they run on the
 * governor's interactive lane. Matched on the dotted path from the API root.
 */
export const INTERACTIVE_HOST_METHODS: ReadonlySet<string> = new Set([
  'ui.confirm',
  'ui.pickUser',
  'ui.pickAsset',
  'ui.pickTicket',
  'ui.pickDate',
]);

export class BridgeGovernor {
  private calls: number[] = [];
  private inFlight = 0;
  private interactivePending = 0;

  constructor(private readonly opts: GovernorOptions = DEFAULT_GOVERNOR_OPTIONS) {}

  async run<T>(fn: () => Promise<T>): Promise<T> {
    this.admitRate();
    if (this.inFlight >= this.opts.maxInFlight) {
      throw new PluginApiError('rate_limited', `bridge in-flight limit exceeded (${this.opts.maxInFlight})`);
    }
    this.recordCall();
    this.inFlight += 1;
    try {
      return await this.withTimeout(fn());
    } finally {
      this.inFlight -= 1;
    }
  }

  /**
   * Run a call that waits on the user (a host dialog). Counted in the rate
   * window like any call; not timed out and not counted in-flight, since a
   * person may take minutes. A second one while one is pending is refused.
   */
  async runInteractive<T>(fn: () => Promise<T>): Promise<T> {
    this.admitRate();
    const max = this.opts.maxInteractivePending ?? 1;
    if (this.interactivePending >= max) {
      throw new PluginApiError(
        'rate_limited',
        'a host dialog is already open for this plugin; wait for it to close before opening another',
      );
    }
    this.recordCall();
    this.interactivePending += 1;
    try {
      return await fn();
    } finally {
      this.interactivePending -= 1;
    }
  }

  private admitRate(): void {
    const windowStart = Date.now() - this.opts.windowMs;
    this.calls = this.calls.filter((t) => t > windowStart);
    if (this.calls.length >= this.opts.maxCallsPerWindow) {
      throw new PluginApiError(
        'rate_limited',
        `bridge rate limit exceeded (${this.opts.maxCallsPerWindow}/${this.opts.windowMs}ms)`,
      );
    }
  }

  private recordCall(): void {
    this.calls.push(Date.now());
  }

  private withTimeout<T>(p: Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(
        () =>
          reject(
            new PluginApiError('timeout', `bridge call timed out after ${this.opts.callTimeoutMs}ms`),
          ),
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
 *
 * A method whose dotted path (`ui.confirm`) is in `interactive` runs on the
 * governor's interactive lane; everything else on the normal one.
 */
export function governHostApi<T extends object>(
  impl: T,
  governor: BridgeGovernor,
  interactive: ReadonlySet<string> = INTERACTIVE_HOST_METHODS,
  path = '',
): T {
  return new Proxy(impl, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver);
      const name = typeof prop === 'string' ? (path ? `${path}.${prop}` : prop) : '';
      if (typeof value === 'function') {
        const lane = interactive.has(name) ? 'runInteractive' : 'run';
        return (...args: unknown[]) =>
          governor[lane](() =>
            Promise.resolve((value as (...a: unknown[]) => unknown).apply(target, args)),
          );
      }
      if (value && typeof value === 'object') {
        return governHostApi(value as object, governor, interactive, name);
      }
      return value;
    },
  });
}
