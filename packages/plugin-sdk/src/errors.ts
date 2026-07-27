// The typed error a host API call throws when it can't return a value. Unifies
// the surface: denial, rate-limit, timeout, bad input, and upstream failure all
// throw a `PluginApiError` (distinguishable by `code`); `null` is reserved for a
// genuine not-found (a `get` of a resource that doesn't exist).
//
// Bridge note: Comlink's built-in error serialization preserves an Error's
// `name`, `message`, and `stack` but NOT custom fields, so the `code` is carried
// in the `name` (`PluginApiError:<code>`) and recovered on the plugin side with
// `asPluginApiError`.

export type PluginApiErrorCode =
  | 'denied' // the plugin's consented scope doesn't grant this
  | 'not_found' // the addressed resource doesn't exist
  | 'rate_limited' // the bridge governor's call-rate / in-flight cap
  | 'timeout' // the call didn't settle within the governor's budget
  | 'invalid' // bad input (e.g. an unparseable fetch URL)
  | 'upstream'; // the host-side call failed for another reason

const NAME_PREFIX = 'PluginApiError';

export class PluginApiError extends Error {
  readonly code: PluginApiErrorCode;
  constructor(code: PluginApiErrorCode, detail?: string) {
    super(detail ? `${code}: ${detail}` : code);
    // `name` carries the code so it survives Comlink's error serialization.
    this.name = `${NAME_PREFIX}:${code}`;
    this.code = code;
  }
}

const CODES = new Set<PluginApiErrorCode>([
  'denied',
  'not_found',
  'rate_limited',
  'timeout',
  'invalid',
  'upstream',
]);

/**
 * Recover a `PluginApiError` from a value caught across the bridge. Comlink
 * re-throws host errors as plain `Error`s (name + message preserved), so match on
 * the `PluginApiError:<code>` name and rebuild a typed error. Returns `null` for
 * anything that isn't a plugin API error, so a plugin can:
 *
 *   catch (e) { const pe = asPluginApiError(e); if (pe?.code === 'denied') … }
 */
export function asPluginApiError(err: unknown): PluginApiError | null {
  if (err instanceof PluginApiError) return err;
  if (
    typeof err === 'object' &&
    err !== null &&
    'name' in err &&
    typeof (err as { name: unknown }).name === 'string'
  ) {
    const name = (err as { name: string }).name;
    if (name.startsWith(`${NAME_PREFIX}:`)) {
      const code = name.slice(NAME_PREFIX.length + 1) as PluginApiErrorCode;
      if (CODES.has(code)) {
        const message =
          'message' in err && typeof (err as { message: unknown }).message === 'string'
            ? (err as { message: string }).message
            : code;
        // Strip the `code: ` prefix the constructor added, if present.
        const detail = message.startsWith(`${code}: `) ? message.slice(code.length + 2) : undefined;
        return new PluginApiError(code, detail);
      }
    }
  }
  return null;
}
