import { describe, expect, it } from 'vitest'
import { createPinia } from 'pinia'
import { nextTick } from 'vue'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

// en-US is in the startup code; every other locale is a chunk of its own,
// loaded when it becomes the active one.
import { createI18n, localeReady, SUPPORTED_LOCALES } from '@/i18n'

function setUp() {
  const pinia = createPinia()
  const fluent = createI18n(pinia)
  return { fluent, dates: useDateStore(pinia) }
}

describe('locale catalogues', () => {
  it('lists every shipped locale, loaded or not', () => {
    expect(SUPPORTED_LOCALES).toEqual(expect.arrayContaining(['en-AU', 'en-GB', 'en-US', 'fr-FR', 'nl-NL']))
  })

  it('speaks en-US from the start', () => {
    const { fluent } = setUp()
    expect(fluent.format('search-result-group-project')).toBe('Projects')
  })

  it('switches to a locale once its catalogue has loaded', async () => {
    const { fluent, dates } = setUp()
    dates.setUserLocale('fr-FR')
    await nextTick()
    await localeReady()
    expect(fluent.format('search-result-group-project')).toBe('Projets')
  })

  it('falls back to en-US for a key a locale lacks', async () => {
    const { fluent, dates } = setUp()
    dates.setUserLocale('en-AU')
    await nextTick()
    await localeReady()
    expect(fluent.bundles.map((b) => b.locales[0])).toEqual(['en-AU', 'en-US'])
  })

  it('stays in en-US for a locale it has no catalogue for', async () => {
    const { fluent, dates } = setUp()
    dates.setUserLocale('de-DE')
    await nextTick()
    await localeReady()
    expect(fluent.format('search-result-group-project')).toBe('Projects')
  })
})
