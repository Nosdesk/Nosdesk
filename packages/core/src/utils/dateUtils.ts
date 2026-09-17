/**
 * Date / time formatting, locale + timezone aware.
 *
 * Every user-visible formatter reads `globalConfig.defaultLocale`
 * and `globalConfig.defaultTimezone`, both seeded from
 * `dateStore.loadFromUser` after `/auth/me` and updated via the
 * settings picker. Flipping the picker re-renders dates and
 * relative-time strings in the chosen locale + zone.
 *
 * Implementation:
 *  - `Intl.DateTimeFormat` for absolute dates / times: correct
 *    localized month names, 12h/24h conventions, day-month order,
 *    and range collapsing (`formatDateRange`), with no dependency.
 *  - `Intl.RelativeTimeFormat` for "5 minutes ago" / "yesterday"
 *    style strings, same locale awareness for free.
 *  - Fluent (`utils/i18n` via the `t` callable returned by
 *    `useFluent`) for the connecting copy we author ourselves,
 *    e.g. inbox-time's "Yesterday at {time}". Module functions
 *    can't call `useFluent()` directly because that's a Vue
 *    composable; callers that need localized connecting copy pass
 *    a translator to `formatInboxTime`.
 *
 * Absolute formats are named presets (`DatePreset`), never pattern
 * strings: a pattern fixes the field order, which is the locale's
 * to decide. Calendar arithmetic lives in `dateMath`.
 */

import { getLocalTimeZone } from '@internationalized/date'

// ============================================
// CONFIGURATION
// ============================================

export interface DateConfig {
  /** IANA timezone, e.g. `Australia/Sydney`. */
  defaultTimezone: string
  /** BCP-47 locale tag, e.g. `en-AU`. */
  defaultLocale: string
}

const DEFAULT_CONFIG: DateConfig = {
  defaultTimezone: 'UTC',
  defaultLocale: 'en-US',
}

let globalConfig: DateConfig = { ...DEFAULT_CONFIG }

export function setDateConfig(config: Partial<DateConfig>): void {
  globalConfig = { ...globalConfig, ...config }
}

export function getDateConfig(): DateConfig {
  return { ...globalConfig }
}

// ============================================
// CORE PARSE
// ============================================

/**
 * Parse a backend-issued ISO string (TIMESTAMPTZ, has a zone
 * marker), a TIMESTAMP-without-zone (NaiveDateTime, treated as UTC)
 * or a bare `YYYY-MM-DD` (UTC midnight) into a `Date`. Zoneless
 * input is completed to the full ISO form before parsing, which is
 * the one shape every engine's `Date` parser agrees on.
 */
export function parseDate(dateString: string | Date | null | undefined): Date | null {
  if (!dateString) return null

  let date: Date
  if (typeof dateString === 'string') {
    const hasZone =
      dateString.endsWith('Z') ||
      dateString.includes('+') ||
      dateString.includes('-', 10)
    const normalized = hasZone
      ? dateString
      : dateString.length === 10
        ? `${dateString}T00:00:00Z`
        : `${dateString}Z`
    date = new Date(normalized)
  } else {
    date = dateString
  }

  if (isNaN(date.getTime())) {
    console.error('Invalid date:', dateString)
    return null
  }

  return date
}

// ============================================
// INTL HELPERS
// ============================================

/**
 * Build an `Intl.DateTimeFormat` against the current locale +
 * timezone (or an override). The format reads `globalConfig` at
 * call time so a locale flip is picked up without re-instantiating.
 */
function intlFormatter(
  opts: Intl.DateTimeFormatOptions,
  overrideTimezone?: string,
): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(globalConfig.defaultLocale, {
    ...opts,
    timeZone: overrideTimezone ?? globalConfig.defaultTimezone,
  })
}

/**
 * Named absolute formats. Field order and separators come from the
 * locale (`en-US` "Sep 17", `en-AU` "17 Sep", `fr` "17 sept."), so a
 * preset says which fields, never how they are arranged.
 */
export const DATE_PRESETS = {
  /** "Sep 17, 2026", the default. */
  date: { year: 'numeric', month: 'short', day: 'numeric' },
  /** "Sep 17, 2026, 3:42 PM". */
  dateTime: { year: 'numeric', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' },
  /** "September 17, 2026, 3:42 PM". */
  longDateTime: { year: 'numeric', month: 'long', day: 'numeric', hour: 'numeric', minute: '2-digit' },
  /** "Sep 17". */
  monthDay: { month: 'short', day: 'numeric' },
  /** "Sep 2026". */
  monthYear: { month: 'short', year: 'numeric' },
  /** "Sep". */
  month: { month: 'short' },
  /** "17". */
  day: { day: 'numeric' },
  /** "Thu". */
  weekday: { weekday: 'short' },
} as const satisfies Record<string, Intl.DateTimeFormatOptions>

export type DatePreset = keyof typeof DATE_PRESETS

// ============================================
// ABSOLUTE FORMATTERS
// ============================================

/**
 * Absolute date in the active locale. `preset` picks the fields
 * (default `date`); `timezone` overrides the configured zone, which
 * a caller holding browser-local calendar dates (the Gantt) must do,
 * or a local midnight renders as the previous or next day.
 */
export function formatDate(
  dateString: string | Date | null | undefined,
  preset: DatePreset = 'date',
  timezone?: string,
): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return intlFormatter(DATE_PRESETS[preset], timezone).format(date)
}

/**
 * Inclusive date range with the shared parts collapsed the way the
 * locale does it: "Sep 7 – 13" / "7–13 Sep" / "Aug 28 – Sep 3".
 */
export function formatDateRange(
  from: string | Date | null | undefined,
  to: string | Date | null | undefined,
  preset: DatePreset = 'monthDay',
  timezone?: string,
): string {
  const start = parseDate(from)
  const end = parseDate(to)
  if (!start || !end) return ''
  return intlFormatter(DATE_PRESETS[preset], timezone).formatRange(start, end)
}

export function formatDateTime(
  dateString: string | Date | null | undefined,
  timezone?: string,
): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return intlFormatter(
    {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    },
    timezone,
  ).format(date)
}

export function formatTime(
  dateString: string | Date | null | undefined,
  timezone?: string,
): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return intlFormatter(
    { hour: 'numeric', minute: '2-digit' },
    timezone,
  ).format(date)
}

/**
 * Compact date: omits the year if the date is in the current
 * calendar year. "Dec 24" vs "Dec 24, 2024".
 */
export function formatCompactDate(
  dateString: string | Date | null | undefined,
): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return intlFormatter(
    isThisYear(date)
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' },
  ).format(date)
}

export function formatMonthYear(dateString: string | Date): string {
  const date = parseDate(dateString)
  if (!date) return ''
  return intlFormatter({ year: 'numeric', month: 'long' }).format(date)
}

// ============================================
// RELATIVE FORMATTERS
// ============================================

/**
 * Relative-time formatter built on `Intl.RelativeTimeFormat`, so
 * "5 minutes ago" / "yesterday" / "tomorrow" all come out in the
 * active locale automatically. `numeric: 'auto'` is what produces
 * "yesterday" instead of "1 day ago" — preferred at most call
 * sites because it reads more naturally.
 */
export function formatRelativeTime(
  dateString: string | Date | null | undefined,
  options?: {
    addSuffix?: boolean
    includeSeconds?: boolean
  },
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const rtf = new Intl.RelativeTimeFormat(globalConfig.defaultLocale, {
    numeric: 'auto',
  })
  const diffSec = Math.round((Date.now() - date.getTime()) / 1000)
  const sign = -1

  const includeSeconds = options?.includeSeconds ?? false

  if (Math.abs(diffSec) < 60) {
    return includeSeconds
      ? rtf.format(sign * diffSec, 'second')
      : rtf.format(sign * Math.round(diffSec / 60), 'minute')
  }
  const diffMin = Math.round(diffSec / 60)
  if (Math.abs(diffMin) < 60) return rtf.format(sign * diffMin, 'minute')
  const diffHour = Math.round(diffMin / 60)
  if (Math.abs(diffHour) < 24) return rtf.format(sign * diffHour, 'hour')
  const diffDay = Math.round(diffHour / 24)
  if (Math.abs(diffDay) < 30) return rtf.format(sign * diffDay, 'day')
  const diffMonth = Math.round(diffDay / 30)
  if (Math.abs(diffMonth) < 12) return rtf.format(sign * diffMonth, 'month')
  const diffYear = Math.round(diffDay / 365)
  return rtf.format(sign * diffYear, 'year')
}

/**
 * Compact relative formatter for space-constrained UIs ("3m",
 * "2h", "5d"). Symbols are language-neutral so this stays
 * untranslated.
 */
export function formatCompactRelativeTime(
  dateString: string | Date | null | undefined,
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const diffInSeconds = Math.floor((Date.now() - date.getTime()) / 1000)
  if (diffInSeconds < 0) return 'now'
  if (diffInSeconds < 60) return '<1m'
  const diffInMinutes = Math.floor(diffInSeconds / 60)
  if (diffInMinutes < 60) return `${diffInMinutes}m`
  const diffInHours = Math.floor(diffInMinutes / 60)
  if (diffInHours < 24) return `${diffInHours}h`
  const diffInDays = Math.floor(diffInHours / 24)
  if (diffInDays < 7) return `${diffInDays}d`
  const diffInWeeks = Math.floor(diffInDays / 7)
  if (diffInWeeks < 4) return `${diffInWeeks}w`
  const diffInMonths = Math.floor(diffInDays / 30)
  if (diffInMonths < 12) return `${diffInMonths}mo`
  const diffInYears = Math.floor(diffInDays / 365)
  return `${diffInYears}y`
}

/**
 * Inbox / notification timestamp. Tiered formatter that mirrors
 * the convention every modern inbox uses:
 *
 *   < 1 min       Just now           (relative)
 *   < 60 min      5 minutes ago      (Intl.RelativeTimeFormat)
 *   today         3:42 PM            (Intl time-only)
 *   yesterday     Yesterday at 3:42 PM
 *   < 7 days      Mon at 3:42 PM
 *   this year     Mar 12
 *   older         Mar 12, 2024
 *
 * Connecting copy ("Yesterday at", "Mon at") is keyed via Fluent;
 * the caller passes a translator so the function stays usable
 * outside Vue components (utilities, services). Pass a no-op
 * translator (e.g. `(k) => k`) and the bare time string is
 * returned as a fallback.
 */
export function formatInboxTime(
  dateString: string | Date | null | undefined,
  t?: (key: string, args?: Record<string, string>) => string,
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / 60_000)

  const translate = t ?? ((k, args) => formatPlaceholder(k, args))

  if (diffMins < 1) return translate('inbox-time-just-now')
  if (diffMins < 60) return formatRelativeTime(date)

  const time = intlFormatter({ hour: 'numeric', minute: '2-digit' }).format(date)
  const ts = date.getTime()
  const startOfToday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).getTime()
  const startOfYesterday = startOfToday - 86_400_000
  const startOfWeek = startOfToday - 6 * 86_400_000

  if (ts >= startOfToday) return time
  if (ts >= startOfYesterday) {
    return translate('inbox-time-yesterday', { time })
  }
  if (ts >= startOfWeek) {
    const day = intlFormatter({ weekday: 'short' }).format(date)
    return translate('inbox-time-weekday', { day, time })
  }
  if (date.getFullYear() === now.getFullYear()) {
    return intlFormatter({ month: 'short', day: 'numeric' }).format(date)
  }
  return intlFormatter({
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  }).format(date)
}

/**
 * No-translator fallback used by `formatInboxTime` when called
 * from outside a Vue component: render the FTL placeholder
 * shape ("Yesterday at {time}") as a best-effort English
 * approximation. Callers inside components pass a real
 * translator and never hit this branch.
 */
function formatPlaceholder(key: string, args?: Record<string, string>): string {
  const fallbacks: Record<string, string> = {
    'inbox-time-just-now': 'Just now',
    'inbox-time-yesterday': `Yesterday at ${args?.time ?? ''}`,
    'inbox-time-weekday': `${args?.day ?? ''} at ${args?.time ?? ''}`,
  }
  return fallbacks[key] ?? key
}

/**
 * Smart date: relative for the recent N days, absolute after.
 */
export function formatSmartDate(
  dateString: string | Date | null | undefined,
  cutoffDays: number = 7,
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const diffInMs = Date.now() - date.getTime()
  const diffInDays = Math.floor(diffInMs / (1000 * 60 * 60 * 24))

  if (diffInDays < cutoffDays) {
    return formatRelativeTime(dateString)
  }
  return formatDate(dateString)
}

/**
 * Relative for the recent N days, compact absolute after. Kept under
 * its historical name so existing call sites don't break.
 */
export function formatCleanRelativeTime(
  dateString: string | Date | null | undefined,
  cutoffDays: number = 7,
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  const diffInMs = Date.now() - date.getTime()
  const diffInDays = Math.floor(diffInMs / (1000 * 60 * 60 * 24))

  if (diffInDays < cutoffDays) {
    return formatRelativeTime(dateString)
  }
  return formatCompactDate(dateString)
}

// ============================================
// MISC
// ============================================

export function getUserTimezone(): string {
  return getLocalTimeZone()
}

/**
 * Backwards-compat: callers used this to convert a UTC date into
 * the user's timezone for downstream formatting. Now formatters
 * take care of timezone themselves, so this is a no-op that just
 * parses.
 */
export function toUserTimezone(
  dateString: string | Date | null | undefined,
  _timezone?: string,
): Date | null {
  return parseDate(dateString)
}

export function isToday(dateString: string | Date | null | undefined): boolean {
  const date = parseDate(dateString)
  if (!date) return false
  // Use the active timezone for "today" — a user in Sydney viewing
  // a UTC timestamp at 14:00 UTC (= 1am next day Sydney time)
  // should see "today" as the Sydney day, not the UTC day.
  const fmt = intlFormatter({ year: 'numeric', month: '2-digit', day: 'numeric' })
  return fmt.format(date) === fmt.format(new Date())
}

export function isThisYear(dateString: string | Date | null | undefined): boolean {
  const date = parseDate(dateString)
  if (!date) return false
  const fmt = intlFormatter({ year: 'numeric' })
  return fmt.format(date) === fmt.format(new Date())
}

export function getCurrentUTCDateTime(): string {
  return new Date().toISOString()
}
