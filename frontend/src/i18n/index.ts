/**
 * Fluent message catalogue loader for the frontend.
 *
 * Eagerly imports every shared `.ftl` file from the workspace-root
 * `i18n/locales/` tree at build time so the production bundle ships
 * its own translations. The same files feed the Rust backend via
 * `include_str!`; keeping the two in lockstep is the whole reason
 * the directory lives at the workspace root instead of inside
 * either package.
 *
 * Active locale follows `dateStore.locale` — set by `/auth/me` after
 * login (see `dateStore.loadFromUser`). When it changes,
 * `fluent.bundles` is reassigned so the next render uses the new
 * catalogue. Vue components consume strings via `useFluent().$t`
 * (composable) or `$t('key')` in template (global property), both
 * provided by `fluent-vue`.
 *
 * Fallback: every lookup falls through to `DEFAULT_LOCALE` (en-US)
 * if a key is missing in the active catalogue, so partial
 * translations (e.g. en-AU only overrides a few keys) work without
 * the gap showing as `{key-name}` bracketed.
 */

import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue, type FluentVue } from 'fluent-vue'
import { watch } from 'vue'
import type { Pinia } from 'pinia'

import { useDateStore } from '@/stores/dateStore'

const DEFAULT_LOCALE = 'en-US'

// Vite eager-glob: every `.ftl` file under workspace-root i18n/
// gets bundled as a raw string. The relative path is from this
// file (`frontend/src/i18n/index.ts`); three `..` reach workspace
// root.
const ftlFiles = import.meta.glob('../../../i18n/locales/*/main.ftl', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>

function buildBundle(locale: string, source: string): FluentBundle {
  // useIsolating=false disables the U+2068/U+2069 directional
  // markers Fluent normally wraps interpolated values in. Our
  // shipped locales are all LTR; the isolates would otherwise
  // appear as bare whitespace artifacts in screenshots and
  // snapshot tests.
  const bundle = new FluentBundle(locale, { useIsolating: false })
  const errors = bundle.addResource(new FluentResource(source))
  if (errors.length > 0) {
    // Loud in dev so authors notice typos in `.ftl` files;
    // production still ships whatever parsed successfully.
    console.warn(`[i18n] Fluent parse errors for ${locale}:`, errors)
  }
  return bundle
}

const bundles: Record<string, FluentBundle> = {}
for (const [path, source] of Object.entries(ftlFiles)) {
  // e.g. `../../../i18n/locales/en-AU/main.ftl` -> `en-AU`
  const match = /\/locales\/([^/]+)\/main\.ftl$/.exec(path)
  if (match) {
    bundles[match[1]] = buildBundle(match[1], source)
  }
}

/**
 * Bundles to pass to fluent-vue for a given locale. The first
 * entry is the requested locale; the second is the default,
 * which serves as the fallback for any key the primary lacks.
 */
function bundlesForLocale(locale: string): FluentBundle[] {
  const primary = bundles[locale]
  const fallback = bundles[DEFAULT_LOCALE]
  if (primary && primary !== fallback) return [primary, fallback]
  return fallback ? [fallback] : []
}

/**
 * Build the fluent-vue plugin instance and bind its active
 * bundle to `dateStore.locale`. Call once during app bootstrap;
 * the returned plugin is registered via `app.use(...)`.
 *
 * Requires the Pinia instance up-front (rather than fetching it
 * via the singleton) because i18n is created before `app.use(pinia)`
 * settles on some plugin orderings.
 */
export function createI18n(pinia: Pinia): FluentVue {
  const dateStore = useDateStore(pinia)
  const fluent = createFluentVue({
    bundles: bundlesForLocale(dateStore.locale),
  })

  watch(
    () => dateStore.locale,
    (locale) => {
      fluent.bundles = bundlesForLocale(locale)
    },
  )

  // Stash the instance so non-component callers (the router, Pinia
  // stores, standalone utilities) can translate without going through
  // useFluent(), which requires an active Vue inject context.
  activeFluent = fluent
  return fluent
}

// Module-level handle to the active fluent-vue instance. Set by
// createI18n during bootstrap; null until then. Callers go through
// the `translate` helper below rather than touching this directly.
let activeFluent: FluentVue | null = null

/**
 * Translate a Fluent key from non-component code (the router,
 * services). Falls back to the supplied `fallback` string (or the
 * key itself) if i18n hasn't been initialised yet, which keeps unit
 * tests and pre-bootstrap call sites from crashing. Args use
 * `FluentVariable`-compatible primitives.
 *
 * Prefer `useFluent().$t` inside components; this is the escape
 * hatch for code paths that run outside a Vue setup context.
 */
export function translate(
  key: string,
  args?: Record<string, string | number>,
  fallback?: string,
): string {
  if (!activeFluent) return fallback ?? key
  const out = activeFluent.format(key, args)
  // fluent-vue returns the bare key id when the message is missing from
  // every bundle. Prefer the caller's fallback copy in that case so a
  // not-yet-bundled or mistyped key degrades to readable English instead
  // of surfacing "some-key-name" to the user.
  if (out === key && fallback !== undefined) return fallback
  return out
}

/** Locales we ship catalogues for. Exposed so the settings UI
 * picker can render the list without duplicating the constant. */
export const SUPPORTED_LOCALES = Object.keys(bundles).sort()
