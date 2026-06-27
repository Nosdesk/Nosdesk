/**
 * Date / locale store.
 *
 * Owns:
 *  - the user's raw timezone preference (`userTimezone`), with
 *    `'system'` meaning "use browser detection"
 *  - the resolved zone that date formatting actually uses
 *    (`effectiveTimezone`)
 *  - the locale UI strings should render in (`locale`)
 *  - the raw locale preference, if any (`userLocale`), null when
 *    the user is inheriting the site default
 *
 * After `/auth/me` returns, `loadFromUser` seeds all four from the
 * backend-resolved `effective_*` fields plus the raw `timezone` /
 * `locale` prefs. Settings UI binds to the raw prefs; date
 * formatting / `useI18n` bind to the effective ones.
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { setDateConfig, getUserTimezone } from '../utils/dateUtils'

const DEFAULT_LOCALE = 'en-US'

interface UserLocalePrefs {
  locale?: string | null
  timezone?: string | null
  effective_locale?: string | null
  effective_timezone?: string | null
}

export const useDateStore = defineStore('date', () => {
  // Raw user preferences. `'system'` is the sentinel for timezone
  // meaning the user hasn't picked one and we should fall through
  // to the browser. `userLocale === null` means the same thing for
  // locale (inherit from site default).
  const userTimezone = ref<string>('system')
  const userLocale = ref<string | null>(null)

  const browserTimezone = computed(() => getUserTimezone())

  const effectiveTimezone = computed(() => {
    if (userTimezone.value === 'system') {
      return browserTimezone.value
    }
    return userTimezone.value
  })

  // What UI strings render in. Driven by the backend resolver on
  // login; fallback to en-US for the pre-auth shell.
  const locale = ref<string>(DEFAULT_LOCALE)

  function updateGlobalConfig() {
    setDateConfig({
      defaultTimezone: effectiveTimezone.value,
      defaultLocale: locale.value
    })
  }

  function setUserTimezone(timezone: string) {
    userTimezone.value = timezone
    updateGlobalConfig()
  }

  function autoDetectTimezone() {
    userTimezone.value = 'system'
    updateGlobalConfig()
  }

  /**
   * Set the user's explicit locale preference. Pass `null` to
   * revert to "inherit from site default" — the next `/me`
   * response repopulates `locale` from the resolver chain.
   *
   * Optimistic local update only; persisting to the backend is
   * the caller's job (PATCH /users/:uuid with `locale`).
   */
  function setUserLocale(next: string | null) {
    userLocale.value = next
    if (next) {
      locale.value = next
    }
    updateGlobalConfig()
  }

  /**
   * Seed all four refs from a `/auth/me`-shaped user object.
   *
   * `timezone` / `locale` carry the raw preference (null when the
   * user is inheriting). `effective_*` carry the backend's
   * resolved fallback chain (user pref -> site default ->
   * hardcoded). Settings UI binds to the raw prefs so it can show
   * "inheriting" vs "explicit choice"; rendering binds to
   * `locale` / `effectiveTimezone`.
   */
  function loadFromUser(user: UserLocalePrefs) {
    userTimezone.value = user.timezone ?? 'system'
    userLocale.value = user.locale ?? null
    locale.value = user.effective_locale ?? user.locale ?? DEFAULT_LOCALE
    updateGlobalConfig()
  }

  // Pre-auth boot config so the login shell formats dates sensibly
  // via browser detection.
  updateGlobalConfig()

  return {
    userTimezone,
    userLocale,
    browserTimezone,
    effectiveTimezone,
    locale,
    setUserTimezone,
    setUserLocale,
    autoDetectTimezone,
    loadFromUser
  }
})
