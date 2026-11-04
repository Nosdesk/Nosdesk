/**
 * Fluent message catalogue loader for the frontend.
 *
 * Bundles the shared `.ftl` files from the workspace-root
 * `i18n/locales/` tree at build time so the production bundle ships
 * its own translations. en-US, the fallback every lookup ends at, is
 * in the startup code; each other locale is its own chunk, loaded
 * when it becomes the active one. The same files feed the Rust backend via
 * `include_str!`; keeping the two in lockstep is the whole reason
 * the directory lives at the workspace root instead of inside
 * either package.
 *
 * Active locale follows `dateStore.locale`, set by `/auth/me` after
 * login (see `dateStore.loadFromUser`). When it changes, its
 * catalogue loads and `fluent.bundles` is reassigned so the next
 * render uses it; until then the current one stays. Vue components consume strings via `useFluent().$t`
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

import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { setActiveFluent } from '@nosdesk/core/i18n'

// `translate()` now lives in @nosdesk/core (headless, so the mobile app shares
// it). Re-exported here so the many `import { translate } from '@/i18n'` call
// sites keep working; this file still owns catalogue loading + `createI18n`.
export { translate } from '@nosdesk/core/i18n'

const DEFAULT_LOCALE = 'en-US'

// Vite globs over the workspace-root i18n/ tree: the relative path is
// from this file (`frontend/src/i18n/index.ts`); three `..` reach
// workspace root. en-US comes in as a raw string with the startup
// code; the rest as loaders, one chunk per locale.
const DEFAULT_SOURCE = import.meta.glob('../../../i18n/locales/en-US/main.ftl', {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>
const ftlLoaders = import.meta.glob(['../../../i18n/locales/*/main.ftl', '!../../../i18n/locales/en-US/main.ftl'], {
  query: '?raw',
  import: 'default',
}) as Record<string, () => Promise<string>>

/** `../../../i18n/locales/en-AU/main.ftl` -> `en-AU` */
function localeOf(path: string): string | null {
  return /\/locales\/([^/]+)\/main\.ftl$/.exec(path)?.[1] ?? null
}

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
for (const [path, source] of Object.entries(DEFAULT_SOURCE)) {
  const locale = localeOf(path)
  if (locale) bundles[locale] = buildBundle(locale, source)
}

const loaders: Record<string, () => Promise<string>> = {}
for (const [path, load] of Object.entries(ftlLoaders)) {
  const locale = localeOf(path)
  if (locale) loaders[locale] = load
}

const loading = new Map<string, Promise<void>>()

/** Load a locale's catalogue once. An unknown or failed one leaves en-US. */
function loadLocale(locale: string): Promise<void> {
  if (bundles[locale] || !loaders[locale]) return Promise.resolve()
  let pending = loading.get(locale)
  if (!pending) {
    pending = loaders[locale]()
      .then((source) => {
        bundles[locale] = buildBundle(locale, source)
      })
      .catch((error) => {
        loading.delete(locale)
        console.warn(`[i18n] Could not load the ${locale} catalogue:`, error)
      })
    loading.set(locale, pending)
  }
  return pending
}

let activeLoad: Promise<void> = Promise.resolve()

/**
 * Settles once the active locale's catalogue is in, so a first paint
 * after it speaks that language rather than en-US. Never rejects.
 */
export function localeReady(): Promise<void> {
  return activeLoad
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

  // Swap in a locale's bundles once its catalogue is in, unless the
  // locale changed again while it loaded.
  const activate = (locale: string) => {
    activeLoad = loadLocale(locale).then(() => {
      if (dateStore.locale === locale) fluent.bundles = bundlesForLocale(locale)
    })
  }
  activate(dateStore.locale)

  // `<html lang>` follows the active locale so assistive tech picks the
  // right voice and hyphenation; the static `lang="en"` in index.html is
  // the pre-boot fallback.
  const applyLang = (locale: string) => {
    if (typeof document !== 'undefined') document.documentElement.lang = locale
  }
  applyLang(dateStore.locale)

  watch(
    () => dateStore.locale,
    (locale) => {
      activate(locale)
      applyLang(locale)
    },
  )

  // Register the instance with @nosdesk/core so non-component callers (the
  // router, Pinia stores, standalone utilities) can `translate()` without an
  // active Vue inject context.
  setActiveFluent(fluent)
  return fluent
}

/** Locales we ship catalogues for. Exposed so the settings UI
 * picker can render the list without duplicating the constant. */
export const SUPPORTED_LOCALES = [...Object.keys(bundles), ...Object.keys(loaders)].sort()
