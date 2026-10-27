/**
 * Cloud trial reminders for the workspace owner: what to show, worded how,
 * and whether the owner already dismissed it.
 *
 * The input is the billing projection on the owner's `/api/me/workspaces`
 * row. Only `status === 'trialing'` with a `trial_end` shows anything; every
 * other state (paid, an unknown status, no billing at all) is `null`, and the
 * callers render nothing for `null`.
 *
 * Tiers come from the calendar days left in the viewer's local zone, so a
 * trial ending in two hours is "today" and an extension moves the tier back:
 *   - more than 7 days: `quiet`, the menu line only
 *   - 7 to 4 days: `soon`, an info banner
 *   - 3 to 0 days: `imminent`, a warning banner
 *   - `cancel_at` set: `cancelled`, a neutral banner that never escalates
 *
 * Pure apart from the storage helpers, which take the `Storage` to use so the
 * tests hand in their own.
 */
import type { WorkspaceBilling } from '@nosdesk/core/types/workspace'

export type TrialTier = 'quiet' | 'soon' | 'imminent' | 'cancelled'

export interface TrialNotice {
  tier: TrialTier
  /** Calendar days from today to the trial's last day, 0 = today. */
  days: number
  /** The raw `trial_end`, part of the dismissal key. */
  trialEnd: string
  /** The raw `cancel_at`, set only for the `cancelled` tier. */
  cancelAt: string | null
  /** "Thursday, November 5" (year added when it differs from now's). */
  dateLong: string
  /** "Nov 5" (year added when it differs from now's). */
  dateShort: string
}

const DAY_MS = 86_400_000

function parse(value: string | null | undefined): Date | null {
  if (!value) return null
  const d = new Date(value)
  return Number.isNaN(d.getTime()) ? null : d
}

/** Whole calendar days from `now` to `end` in the local zone. Rounded, not
 *  floored, so a day that is 23 or 25 hours long across a DST change still
 *  counts as one. */
export function calendarDaysBetween(now: Date, end: Date): number {
  const a = new Date(now)
  a.setHours(0, 0, 0, 0)
  const b = new Date(end)
  b.setHours(0, 0, 0, 0)
  return Math.round((b.getTime() - a.getTime()) / DAY_MS)
}

function formatDay(date: Date, now: Date, locale: string, long: boolean): string {
  const opts: Intl.DateTimeFormatOptions = long
    ? { weekday: 'long', day: 'numeric', month: 'long' }
    : { day: 'numeric', month: 'short' }
  if (date.getFullYear() !== now.getFullYear()) opts.year = 'numeric'
  try {
    return new Intl.DateTimeFormat(locale, opts).format(date)
  } catch {
    // An unknown locale tag; the default locale still gives a real date.
    return new Intl.DateTimeFormat(undefined, opts).format(date)
  }
}

/** What the owner should see for this billing state, or `null` for nothing. */
export function trialNotice(
  billing: WorkspaceBilling | null | undefined,
  now: Date,
  locale: string,
): TrialNotice | null {
  if (!billing || billing.status !== 'trialing') return null
  const end = parse(billing.trial_end)
  if (!end || !billing.trial_end) return null
  const days = calendarDaysBetween(now, end)
  // A trial that already ended but still reads `trialing` is a stale push;
  // the next one from Cloud corrects it, and until then there is nothing
  // true to say.
  if (days < 0) return null
  const cancelAt = billing.cancel_at && parse(billing.cancel_at) ? billing.cancel_at : null
  const tier: TrialTier = cancelAt ? 'cancelled' : days > 7 ? 'quiet' : days >= 4 ? 'soon' : 'imminent'
  return {
    tier,
    days,
    trialEnd: billing.trial_end,
    cancelAt,
    dateLong: formatDay(end, now, locale, true),
    dateShort: formatDay(end, now, locale, false),
  }
}

/** Banner tone, or `null` for the tier that has no banner. */
export function bannerTone(notice: TrialNotice): 'info' | 'warning' | 'neutral' | null {
  switch (notice.tier) {
    case 'soon':
      return 'info'
    case 'imminent':
      return 'warning'
    case 'cancelled':
      return 'neutral'
    default:
      return null
  }
}

type Translate = (key: string, args?: Record<string, string | number>) => string

/** The second line under "Plan and billing" in the owner's menu. */
export function menuLine(notice: TrialNotice, t: Translate): string {
  if (notice.tier === 'cancelled') return t('trial-notice-menu-line-cancelled', { date: notice.dateShort })
  return t('trial-notice-menu-line', { days: notice.days, date: notice.dateShort })
}

/** The banner's bold title and the sentence after it; `null` for `quiet`. */
export function bannerCopy(notice: TrialNotice, t: Translate): { title: string; body: string } | null {
  const args = { days: notice.days, date: notice.dateLong }
  switch (notice.tier) {
    case 'soon':
      return { title: t('trial-notice-soon-title', args), body: t('trial-notice-soon-body') }
    case 'imminent':
      return { title: t('trial-notice-imminent-title', args), body: t('trial-notice-imminent-body') }
    case 'cancelled':
      return { title: t('trial-notice-cancelled-title', args), body: t('trial-notice-cancelled-body') }
    default:
      return null
  }
}

// Dismissal. One key per workspace, person, trial end and tier, so a new
// trial end (an extension) or a new tier starts fresh:
//   soon       stays dismissed until the tier changes
//   imminent   comes back whenever the day count changes
//   cancelled  shows once per cancel_at value

const KEY_PREFIX = 'nosdesk-trial-notice'

export function dismissalKey(notice: TrialNotice, workspace: string, user: string): string | null {
  const base = `${KEY_PREFIX}:${workspace}:${user}:${notice.trialEnd}`
  switch (notice.tier) {
    case 'soon':
      return `${base}:soon`
    case 'imminent':
      return `${base}:imminent`
    case 'cancelled':
      return `${base}:cancel:${notice.cancelAt}`
    default:
      return null
  }
}

function defaultStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

export function isDismissed(
  notice: TrialNotice,
  workspace: string,
  user: string,
  storage: Storage | null = defaultStorage(),
): boolean {
  const key = dismissalKey(notice, workspace, user)
  if (!key || !storage) return false
  let stored: string | null = null
  try {
    stored = storage.getItem(key)
  } catch {
    return false
  }
  if (stored === null) return false
  if (notice.tier === 'imminent') return stored === String(notice.days)
  return true
}

export function recordDismissal(
  notice: TrialNotice,
  workspace: string,
  user: string,
  storage: Storage | null = defaultStorage(),
): void {
  const key = dismissalKey(notice, workspace, user)
  if (!key || !storage) return
  try {
    storage.setItem(key, notice.tier === 'imminent' ? String(notice.days) : '1')
  } catch {
    // Private mode or blocked storage: the banner simply comes back.
  }
}
