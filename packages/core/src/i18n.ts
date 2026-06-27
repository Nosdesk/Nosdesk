/**
 * Headless translation helper for @nosdesk/core.
 *
 * `translate()` lets non-component code (services, stores, types) format a
 * Fluent key without an active Vue inject context. The host app owns catalogue
 * loading and the fluent-vue instance (the web app: frontend/src/i18n); at
 * bootstrap it registers that instance here via `setActiveFluent`, the only
 * coupling between the two halves. Kept framework-shell-free so it travels to
 * the mobile app unchanged.
 */
import type { FluentVue } from 'fluent-vue'

// The active fluent-vue instance, registered by the host app at bootstrap.
// Null until then, so `translate` degrades to the fallback/key and
// pre-bootstrap call sites (and unit tests) don't crash.
let activeFluent: FluentVue | null = null

/** Register the host app's fluent-vue instance. Called once at bootstrap. */
export function setActiveFluent(fluent: FluentVue | null): void {
  activeFluent = fluent
}

/**
 * Translate a Fluent key from non-component code. Falls back to `fallback`
 * (or the key itself) when i18n isn't initialised yet, or when the key is
 * missing from every bundle, so a not-yet-bundled or mistyped key degrades to
 * readable copy instead of surfacing the bare key id. Prefer `useFluent().$t`
 * inside components; this is the escape hatch for code outside a Vue setup.
 */
export function translate(
  key: string,
  args?: Record<string, string | number>,
  fallback?: string,
): string {
  if (!activeFluent) return fallback ?? key
  const out = activeFluent.format(key, args)
  if (out === key && fallback !== undefined) return fallback
  return out
}
