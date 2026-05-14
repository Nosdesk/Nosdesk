/**
 * Date / time utilities, locale + timezone aware.
 *
 * Every user-visible formatter reads `globalConfig.defaultLocale`
 * and `globalConfig.defaultTimezone`, both seeded from
 * `dateStore.loadFromUser` after `/auth/me` and updated via the
 * settings picker. Flipping the picker re-renders dates and
 * relative-time strings in the chosen locale + zone.
 *
 * Implementation:
 *  - `Intl.DateTimeFormat` for absolute dates / times — gives
 *    correct localized month names, 12h/24h conventions, day-
 *    month order, etc., without a dependency.
 *  - `Intl.RelativeTimeFormat` for "5 minutes ago" / "yesterday"
 *    style strings — same locale awareness for free.
 *  - Fluent (`utils/i18n` via the `t` callable returned by
 *    `useFluent`) for the connecting copy we author ourselves,
 *    e.g. inbox-time's "Yesterday at {time}". Module functions
 *    can't call `useFluent()` directly because that's a Vue
 *    composable; callers that need localized connecting copy use
 *    the `formatInboxTimeI18n` variant that takes a translator.
 *
 * date-fns is retained only for callers that still pass a
 * literal format string (`"MMM d, yyyy"`). We translate the
 * five strings actually in use across the codebase into
 * `Intl.DateTimeFormat` options below; anything else falls
 * through to date-fns and is locale-agnostic — acceptable for
 * filename stamps, calendar IDs, and similar machine-facing
 * uses, not for new user-facing copy.
 */

import { format, formatDistance, parseISO } from 'date-fns'

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
 * Parse a backend-issued ISO string (TIMESTAMPTZ → has a zone
 * marker) or a TIMESTAMP-without-zone (NaiveDateTime → we treat
 * as UTC by appending Z) into a `Date`.
 */
export function parseDate(dateString: string | Date | null | undefined): Date | null {
  if (!dateString) return null

  let date: Date
  if (typeof dateString === 'string') {
    const normalized =
      dateString.endsWith('Z') ||
      dateString.includes('+') ||
      dateString.includes('-', 10)
        ? dateString
        : dateString + 'Z'
    date = parseISO(normalized)
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
 * Map the small set of date-fns format strings used across the
 * codebase to `Intl.DateTimeFormat` options. Returns `null` for
 * unrecognised strings so the caller can fall through to
 * date-fns (locale-agnostic — fine for filename stamps).
 */
function presetForFormatString(s: string): Intl.DateTimeFormatOptions | null {
  switch (s) {
    case 'MMM d':
      return { month: 'short', day: 'numeric' }
    case 'MMM d, yyyy':
      return { year: 'numeric', month: 'short', day: 'numeric' }
    case 'MMM d, yyyy h:mm a':
      return {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      }
    case 'MMMM d, yyyy':
      return { year: 'numeric', month: 'long', day: 'numeric' }
    case 'MMMM d, yyyy h:mm a':
      return {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      }
    case 'h:mm a':
      return { hour: 'numeric', minute: '2-digit' }
    case 'MMMM yyyy':
      return { year: 'numeric', month: 'long' }
    default:
      return null
  }
}

// ============================================
// ABSOLUTE FORMATTERS
// ============================================

/**
 * Default absolute-date formatter. `formatString` is recognised
 * for the small set of patterns we map to Intl options (so
 * existing call sites stay locale-aware); unrecognised strings
 * fall through to date-fns. `timezone` overrides the global.
 */
export function formatDate(
  dateString: string | Date | null | undefined,
  formatString?: string,
  timezone?: string,
): string {
  const date = parseDate(dateString)
  if (!date) return ''

  if (formatString) {
    const preset = presetForFormatString(formatString)
    if (preset) {
      return intlFormatter(preset, timezone).format(date)
    }
    // Fall through to date-fns for unrecognised patterns. These
    // are machine-facing (filenames, calendar IDs) — not user
    // copy — so locale-agnostic output is the right tradeoff.
    try {
      return format(date, formatString)
    } catch (error) {
      console.error('formatDate: date-fns failed', error)
      return ''
    }
  }

  // No format string: short date in the active locale + zone.
  return intlFormatter(
    { year: 'numeric', month: 'short', day: 'numeric' },
    timezone,
  ).format(date)
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
 * "Clean" relative formatter: same as `formatRelativeTime` since
 * we now use `Intl.RelativeTimeFormat` directly (the old version
 * filtered out date-fns's "about" prefix manually). Kept under
 * the old name so existing call sites don't break.
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
  return Intl.DateTimeFormat().resolvedOptions().timeZone
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

export function formatForCalendar(date: Date): string {
  // ISO calendar key — must stay locale-agnostic.
  return format(date, 'yyyy-MM-dd')
}

export function formatForFilename(date: Date = new Date()): string {
  // ISO filename stamp — must stay locale-agnostic.
  return format(date, 'yyyy-MM-dd-HHmmss')
}

export function getCurrentUTCDateTime(): string {
  return new Date().toISOString()
}

export function formatDistanceBetween(
  startDate: string | Date,
  endDate: string | Date,
  options?: { addSuffix?: boolean },
): string {
  const start = parseDate(startDate)
  const end = parseDate(endDate)
  if (!start || !end) return ''
  try {
    return formatDistance(start, end, options)
  } catch (error) {
    console.error('formatDistanceBetween: date-fns failed', error)
    return ''
  }
}
