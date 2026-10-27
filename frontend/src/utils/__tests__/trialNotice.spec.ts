import { describe, expect, it } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import type { WorkspaceBilling } from '@nosdesk/core/types/workspace'
import enUS from '../../../../i18n/locales/en-US/main.ftl?raw'
import frFR from '../../../../i18n/locales/fr-FR/main.ftl?raw'
import nlNL from '../../../../i18n/locales/nl-NL/main.ftl?raw'
import {
  bannerCopy,
  bannerTone,
  calendarDaysBetween,
  dismissalKey,
  isDismissed,
  menuLine,
  recordDismissal,
  trialNotice,
  type TrialNotice,
} from '@/utils/trialNotice'

// Dates are built in local time and handed over as RFC 3339, so the day
// counts hold in any zone the suite runs in.

/** Monday 2 November 2026, 10:30 local. */
const NOW = new Date(2026, 10, 2, 10, 30)

/** A trial that ends `days` calendar days after NOW, at 17:00 local. */
function endIn(days: number, hour = 17): string {
  return new Date(2026, 10, 2 + days, hour).toISOString()
}

function trialing(days: number, extra: Partial<WorkspaceBilling> = {}): WorkspaceBilling {
  return { status: 'trialing', trial_end: endIn(days), cancel_at: null, ...extra }
}

function notice(billing: WorkspaceBilling | null | undefined, now = NOW, locale = 'en-US') {
  return trialNotice(billing, now, locale)
}

function translator(source: string, locale: string) {
  const bundle = new FluentBundle(locale, { useIsolating: false })
  bundle.addResource(new FluentResource(source))
  return (key: string, args?: Record<string, string | number>) => {
    const msg = bundle.getMessage(key)
    if (!msg?.value) return `{${key}}`
    return bundle.formatPattern(msg.value, args)
  }
}
const t = translator(enUS, 'en-US')

/** An in-memory Storage, plus one that throws on every call. */
function memoryStorage(): Storage {
  const map = new Map<string, string>()
  return {
    get length() {
      return map.size
    },
    clear: () => map.clear(),
    getItem: (k) => map.get(k) ?? null,
    key: (i) => [...map.keys()][i] ?? null,
    removeItem: (k) => void map.delete(k),
    setItem: (k, v) => void map.set(k, String(v)),
  }
}
const brokenStorage: Storage = {
  length: 0,
  clear: () => {},
  key: () => null,
  getItem: () => {
    throw new Error('blocked')
  },
  setItem: () => {
    throw new Error('blocked')
  },
  removeItem: () => {},
}

describe('trialNotice: what shows', () => {
  it.each([
    [14, 'quiet'],
    [8, 'quiet'],
    [7, 'soon'],
    [4, 'soon'],
    [3, 'imminent'],
    [1, 'imminent'],
    [0, 'imminent'],
  ] as const)('%i days left is the %s tier', (days, tier) => {
    const n = notice(trialing(days))
    expect(n?.tier).toBe(tier)
    expect(n?.days).toBe(days)
  })

  it('counts calendar days, so a trial ending in two hours is today', () => {
    const lateNow = new Date(2026, 10, 2, 21, 0)
    expect(notice({ status: 'trialing', trial_end: new Date(2026, 10, 2, 23, 0).toISOString(), cancel_at: null }, lateNow)?.days).toBe(0)
    // 00:30 tomorrow is still tomorrow, not "in 3 hours, so today".
    expect(notice({ status: 'trialing', trial_end: new Date(2026, 10, 3, 0, 30).toISOString(), cancel_at: null }, lateNow)?.days).toBe(1)
  })

  it('counts a day across a DST change as one day', () => {
    // Whatever the local zone, midnight to midnight rounds to whole days.
    expect(calendarDaysBetween(new Date(2026, 2, 28, 12), new Date(2026, 3, 6, 12))).toBe(9)
    expect(calendarDaysBetween(new Date(2026, 9, 24, 12), new Date(2026, 10, 2, 12))).toBe(9)
  })

  it('is the cancelled tier whenever cancel_at is set, at any distance', () => {
    expect(notice(trialing(12, { cancel_at: endIn(12) }))?.tier).toBe('cancelled')
    expect(notice(trialing(5, { cancel_at: endIn(5) }))?.tier).toBe('cancelled')
    expect(notice(trialing(1, { cancel_at: endIn(1) }))?.tier).toBe('cancelled')
  })

  it.each([
    ['a paid plan', { status: 'active', trial_end: endIn(5), cancel_at: null }],
    ['an unknown status', { status: 'paused_for_reasons', trial_end: endIn(5), cancel_at: null }],
    ['no subscription', { status: null, trial_end: null, cancel_at: null }],
    ['trialing without trial_end', { status: 'trialing', trial_end: null, cancel_at: null }],
    ['an unreadable trial_end', { status: 'trialing', trial_end: 'not a date', cancel_at: null }],
    ['a trial that already ended', trialing(-1)],
  ] as const)('shows nothing for %s', (_label, billing) => {
    expect(notice(billing as WorkspaceBilling)).toBeNull()
  })

  it('shows nothing when billing is absent', () => {
    expect(notice(undefined)).toBeNull()
    expect(notice(null)).toBeNull()
  })

  it('only has a banner from seven days out', () => {
    expect(bannerTone(notice(trialing(8))!)).toBeNull()
    expect(bannerTone(notice(trialing(7))!)).toBe('info')
    expect(bannerTone(notice(trialing(3))!)).toBe('warning')
    expect(bannerTone(notice(trialing(10, { cancel_at: endIn(10) }))!)).toBe('neutral')
  })
})

describe('trialNotice: dates', () => {
  it('leaves the year off within the current year', () => {
    const n = notice(trialing(3))!
    expect(n.dateLong).toBe('Thursday, November 5')
    expect(n.dateShort).toBe('Nov 5')
  })

  it('adds the year when the trial ends next year', () => {
    const now = new Date(2026, 11, 29, 9)
    const end = new Date(2027, 0, 4, 17).toISOString()
    const n = notice({ status: 'trialing', trial_end: end, cancel_at: null }, now)!
    expect(n.days).toBe(6)
    expect(n.dateLong).toBe('Monday, January 4, 2027')
    expect(n.dateShort).toBe('Jan 4, 2027')
  })

  it('follows the locale', () => {
    expect(notice(trialing(3), NOW, 'en-AU')!.dateLong).toBe('Thursday 5 November')
    expect(notice(trialing(3), NOW, 'fr-FR')!.dateLong).toBe('jeudi 5 novembre')
    expect(notice(trialing(3), NOW, 'nl-NL')!.dateLong).toBe('donderdag 5 november')
  })
})

describe('trialNotice: copy', () => {
  const line = (billing: WorkspaceBilling) => menuLine(notice(billing)!, t)
  const banner = (billing: WorkspaceBilling) => bannerCopy(notice(billing)!, t)

  it('words the menu line for each distance', () => {
    expect(line(trialing(12))).toBe('Trial ends Nov 14, in 12 days')
    expect(line(trialing(1))).toBe('Trial ends tomorrow, Nov 3')
    expect(line(trialing(0))).toBe('Trial ends today')
    expect(line(trialing(9, { cancel_at: endIn(9) }))).toBe("Trial ends Nov 11, won't continue")
  })

  it('words the seven day banner', () => {
    expect(banner(trialing(7))).toEqual({
      title: 'Your trial ends Monday, November 9, in 7 days.',
      body: 'Your plan and the option to cancel are under Manage billing, in your Nosdesk account. Nothing changes in your workspace before then.',
    })
  })

  it('words the three day banner as the day nears', () => {
    expect(banner(trialing(3))?.title).toBe('Your trial ends in 3 days, on Thursday, November 5.')
    expect(banner(trialing(1))?.title).toBe('Your trial ends tomorrow, Tuesday, November 3.')
    expect(banner(trialing(0))?.title).toBe('Your trial ends today.')
    expect(banner(trialing(0))?.body).toBe(
      "If you haven't decided yet, your plan and the option to cancel are under Manage billing, in your Nosdesk account. Everything you've set up stays as it is until then.",
    )
  })

  it('words the cancelled banner', () => {
    expect(banner(trialing(5, { cancel_at: endIn(5) }))).toEqual({
      title: "Your trial ends Saturday, November 7 and won't continue.",
      body: "There's nothing you need to do. If you'd like to keep Nosdesk after all, you can from Manage billing, in your Nosdesk account, any time before then.",
    })
  })

  it('has no banner copy for the quiet tier', () => {
    expect(banner(trialing(10))).toBeNull()
  })

  it('never mentions a charge or a price', () => {
    const all = [0, 1, 3, 5, 7, 12].flatMap((d) => {
      const n = notice(trialing(d))!
      const b = bannerCopy(n, t)
      return [menuLine(n, t), b?.title ?? '', b?.body ?? '']
    })
    for (const text of all) expect(text).not.toMatch(/charge|\$|price|pay/i)
  })

  it.each([
    ['fr-FR', frFR, 'Votre essai se termine aujourd\'hui.', 'Votre essai se termine demain, mardi 3 novembre.'],
    ['nl-NL', nlNL, 'Uw proefperiode eindigt vandaag.', 'Uw proefperiode eindigt morgen, dinsdag 3 november.'],
  ])('picks the today and tomorrow wording in %s', (locale, source, today, tomorrow) => {
    const tl = translator(source, locale)
    expect(bannerCopy(trialNotice(trialing(0), NOW, locale)!, tl)?.title).toBe(today)
    expect(bannerCopy(trialNotice(trialing(1), NOW, locale)!, tl)?.title).toBe(tomorrow)
  })
})

describe('trialNotice: dismissal', () => {
  const WS = 'ws-uuid'
  const USER = 'user-uuid'
  const at = (days: number, extra: Partial<WorkspaceBilling> = {}) => notice(trialing(days, extra)) as TrialNotice

  it('keys on workspace, person, trial end and tier', () => {
    const end = endIn(5)
    const soon = notice({ status: 'trialing', trial_end: end, cancel_at: null })!
    expect(dismissalKey(soon, WS, USER)).toBe(`nosdesk-trial-notice:${WS}:${USER}:${end}:soon`)
    const cancelled = notice({ status: 'trialing', trial_end: end, cancel_at: end })!
    expect(dismissalKey(cancelled, WS, USER)).toBe(`nosdesk-trial-notice:${WS}:${USER}:${end}:cancel:${end}`)
    expect(dismissalKey(at(10), WS, USER)).toBeNull()
  })

  it('keeps the seven day banner dismissed until the tier changes', () => {
    // One trial end, the clock moving towards it.
    const end = new Date(2026, 10, 9, 17).toISOString()
    const on = (day: number) => notice({ status: 'trialing', trial_end: end, cancel_at: null }, new Date(2026, 10, day, 10))!
    const storage = memoryStorage()
    recordDismissal(on(2), WS, USER, storage)
    expect(isDismissed(on(2), WS, USER, storage)).toBe(true)
    expect(isDismissed(on(5), WS, USER, storage)).toBe(true)
    // Day 3 is a new tier, so it shows.
    expect(on(6).tier).toBe('imminent')
    expect(isDismissed(on(6), WS, USER, storage)).toBe(false)
  })

  it('brings the three day banner back when the day count changes', () => {
    const end = new Date(2026, 10, 9, 17).toISOString()
    const on = (day: number) => notice({ status: 'trialing', trial_end: end, cancel_at: null }, new Date(2026, 10, day, 10))!
    const storage = memoryStorage()
    recordDismissal(on(6), WS, USER, storage)
    expect(isDismissed(on(6), WS, USER, storage)).toBe(true)
    expect(isDismissed(on(7), WS, USER, storage)).toBe(false)
    recordDismissal(on(7), WS, USER, storage)
    expect(isDismissed(on(7), WS, USER, storage)).toBe(true)
    expect(isDismissed(on(9), WS, USER, storage)).toBe(false)
  })

  it('shows the cancelled banner once per cancel_at', () => {
    const storage = memoryStorage()
    const first = at(6, { cancel_at: endIn(6) })
    recordDismissal(first, WS, USER, storage)
    expect(isDismissed(first, WS, USER, storage)).toBe(true)
    expect(isDismissed(at(2, { trial_end: first.trialEnd, cancel_at: endIn(6) }), WS, USER, storage)).toBe(true)
    expect(isDismissed(at(6, { cancel_at: endIn(5) }), WS, USER, storage)).toBe(false)
  })

  it('starts fresh for another person, workspace or trial end', () => {
    const storage = memoryStorage()
    recordDismissal(at(5), WS, USER, storage)
    expect(isDismissed(at(5), WS, 'someone-else', storage)).toBe(false)
    expect(isDismissed(at(5), 'other-ws', USER, storage)).toBe(false)
    expect(isDismissed(at(5, { trial_end: endIn(6) }), WS, USER, storage)).toBe(false)
  })

  it('survives storage that throws or is missing', () => {
    expect(() => recordDismissal(at(5), WS, USER, brokenStorage)).not.toThrow()
    expect(isDismissed(at(5), WS, USER, brokenStorage)).toBe(false)
    expect(isDismissed(at(5), WS, USER, null)).toBe(false)
  })
})
