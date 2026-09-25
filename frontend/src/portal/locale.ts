import { SUPPORTED_LOCALES } from '@/i18n'

/**
 * The shipped locale that best matches the browser's preferences: an exact tag
 * first, then the first shipped variant of the same language, else en-US.
 */
export function browserLocale(preferred: readonly string[] = navigator.languages): string {
  for (const tag of preferred) {
    const exact = SUPPORTED_LOCALES.find((l) => l.toLowerCase() === tag.toLowerCase())
    if (exact) return exact
    const language = tag.split('-')[0].toLowerCase()
    const sameLanguage =
      language === 'en'
        ? 'en-US'
        : SUPPORTED_LOCALES.find((l) => l.split('-')[0].toLowerCase() === language)
    if (sameLanguage) return sameLanguage
  }
  return 'en-US'
}
