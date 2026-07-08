/**
 * URL-bound time range for the dashboard surface.
 *
 * Reads `?range=...` (and `?from=...&to=...` for the Custom case)
 * from the route; writes via `router.replace` so the back button
 * navigates pages rather than range steps. The default range is
 * 7 days. Grain auto-derives from the range; callers can override
 * with `?grain=...` for power use.
 *
 * `compare=prior` is the global compare-to-prior toggle from
 * decision 6 — kept in URL state alongside the range so a
 * Slack-shared link reproduces the entire chrome state.
 *
 * The composable returns reactive computeds; binding inputs to
 * the setters automatically writes back to the URL. No Pinia
 * store, no localStorage — the URL is the single source of truth.
 */
import { computed, type ComputedRef } from 'vue'
import { useRoute, useRouter, type LocationQuery } from 'vue-router'
import { TZDate } from '@date-fns/tz'
import { startOfDay, endOfDay, subDays, subYears } from 'date-fns'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

/** The six preset chips + the Custom escape hatch. */
export type TimeRangePreset =
  | 'today'
  | '7d'
  | '30d'
  | '90d'
  | '1y'
  | '3y'
  | 'custom'

/** Time-bucket resolution. Derived from the range unless overridden. */
export type Grain = 'hour' | 'day' | 'week' | 'month'

const DEFAULT_PRESET: TimeRangePreset = '7d'

/** Map presets to their canonical grain. */
function presetGrain(preset: TimeRangePreset): Grain {
  switch (preset) {
    case 'today':
      return 'hour'
    case '7d':
    case '30d':
    case '90d':
      return 'day'
    case '1y':
    case '3y':
      return 'month'
    case 'custom':
      // Custom defaults to day; the picker can override.
      return 'day'
  }
}

/**
 * Compute the absolute (from, to) window for a preset, with calendar
 * boundaries anchored to the user's timezone `tz` (not the browser's).
 *
 * Calendar-edge presets (today, custom) use `TZDate` so "start of today"
 * is midnight in the user's zone — a Sydney user at 09:00 local gets a
 * window starting at the previous UTC afternoon, matching the backend's
 * tz-aligned buckets. Rolling presets (7d/30d/90d/1y/3y) are pure
 * instants (now minus N), so the zone doesn't affect them.
 * Returned `Date`s carry the correct UTC instant (`toISOString()`).
 */
export function presetWindow(
  preset: TimeRangePreset,
  tz: string,
  custom?: { from?: string; to?: string },
): { from: Date; to: Date } {
  const now = new Date()
  switch (preset) {
    case 'today':
      return { from: startOfDay(new TZDate(now, tz)), to: now }
    case '7d':
      return { from: subDays(now, 7), to: now }
    case '30d':
      return { from: subDays(now, 30), to: now }
    case '90d':
      return { from: subDays(now, 90), to: now }
    case '1y':
      return { from: subYears(now, 1), to: now }
    case '3y':
      return { from: subYears(now, 3), to: now }
    case 'custom': {
      // The range picker stores date-only `YYYY-MM-DD` values. Anchor
      // `from` to the start of its day and `to` to the end of its day,
      // both in the user's zone, so the selected `to` date is inclusive.
      const f = custom?.from ? dayBoundary(custom.from, false, tz) : startOfDay(new TZDate(now, tz))
      const t = custom?.to ? dayBoundary(custom.to, true, tz) : now
      return { from: f, to: t }
    }
  }
}

/** Parse the `YYYY-MM-DD` date part of `value` and anchor it to the
 *  start (`end=false`) or end (`end=true`) of that calendar day in the
 *  user's timezone `tz`. */
function dayBoundary(value: string, end: boolean, tz: string): Date {
  const [y, m, d] = value.slice(0, 10).split('-').map(Number)
  if (!y || !m || !d) return new Date()
  const day = new TZDate(y, m - 1, d, tz)
  return end ? endOfDay(day) : startOfDay(day)
}

/** Return the matching prior window for compare-to-prior overlays. */
export function priorWindow(window: { from: Date; to: Date }): {
  from: Date
  to: Date
} {
  const span = window.to.getTime() - window.from.getTime()
  return {
    from: new Date(window.from.getTime() - span),
    to: new Date(window.from.getTime()),
  }
}

/**
 * Convert a route's query into a typed preset. Unknown / malformed
 * values fall back to the default rather than throwing.
 */
function parsePreset(value: LocationQuery[string]): TimeRangePreset {
  if (typeof value !== 'string') return DEFAULT_PRESET
  switch (value) {
    case 'today':
    case '7d':
    case '30d':
    case '90d':
    case '1y':
    case '3y':
    case 'custom':
      return value
    default:
      return DEFAULT_PRESET
  }
}

function parseGrain(value: LocationQuery[string]): Grain | null {
  if (typeof value !== 'string') return null
  switch (value) {
    case 'hour':
    case 'day':
    case 'week':
    case 'month':
      return value
    default:
      return null
  }
}

/**
 * URL-state-bound time range. Returns reactive computeds; mutating
 * via the setters writes the URL via `router.replace` so back-button
 * history isn't polluted with range steps.
 */
/**
 * Time window. Both ends are ISO-8601 UTC strings: that's the shape
 * the backend's `chrono::DateTime<Utc>` serde deserialiser expects,
 * and it's what axios will url-encode without surprises. Any
 * consumer that needs a Date can do `new Date(window.from)` at the
 * call site; the inverse (everyone calling toISOString() on a
 * shared Date object) was the failure mode the analytics endpoints
 * hit in review.
 */
export interface TimeWindow {
  from: string
  to: string
}

export interface TimeRangeHandle {
  preset: ComputedRef<TimeRangePreset>
  grain: ComputedRef<Grain>
  compare: ComputedRef<boolean>
  customFrom: ComputedRef<string | null>
  customTo: ComputedRef<string | null>
  /** The active window for the dashboard. */
  window: ComputedRef<TimeWindow>
  /**
   * The matching prior-period window (same span, shifted earlier
   * by that span). Always derived from `window`; callers showing a
   * compare overlay read this directly, callers ignoring compare
   * simply don't reference it.
   */
  priorWindow: ComputedRef<TimeWindow>
  setPreset: (next: TimeRangePreset) => void
  setGrainOverride: (next: Grain | null) => void
  setCompare: (next: boolean) => void
  setCustomRange: (from: string, to: string) => void
}

export function useTimeRange(): TimeRangeHandle {
  const route = useRoute()
  const router = useRouter()
  const dateStore = useDateStore()

  const preset = computed<TimeRangePreset>(() => parsePreset(route.query.range))

  const grainOverride = computed<Grain | null>(() => parseGrain(route.query.grain))
  const grain = computed<Grain>(
    () => grainOverride.value ?? presetGrain(preset.value),
  )

  const compare = computed<boolean>(() => route.query.compare === 'prior')

  const customFrom = computed<string | null>(() =>
    typeof route.query.from === 'string' ? route.query.from : null,
  )
  const customTo = computed<string | null>(() =>
    typeof route.query.to === 'string' ? route.query.to : null,
  )

  // `presetWindow` builds Date objects internally (cheaper math);
  // we stringify here so the public surface is ISO-only.
  const window = computed<TimeWindow>(() => {
    const w = presetWindow(preset.value, dateStore.effectiveTimezone, {
      from: customFrom.value ?? undefined,
      to: customTo.value ?? undefined,
    })
    return { from: w.from.toISOString(), to: w.to.toISOString() }
  })

  const priorWindow = computed<TimeWindow>(() => {
    const w = window.value
    const fromMs = Date.parse(w.from)
    const toMs = Date.parse(w.to)
    const span = toMs - fromMs
    return {
      from: new Date(fromMs - span).toISOString(),
      to: new Date(fromMs).toISOString(),
    }
  })

  function writeQuery(patch: Record<string, string | undefined>): void {
    const next: LocationQuery = { ...route.query }
    for (const [key, value] of Object.entries(patch)) {
      if (value === undefined) delete next[key]
      else next[key] = value
    }
    router.replace({ query: next })
  }

  function setPreset(next: TimeRangePreset): void {
    // Switching to a non-custom preset drops the from/to params so a
    // shared link doesn't carry stale custom-range state.
    const patch: Record<string, string | undefined> = { range: next }
    if (next !== 'custom') {
      patch.from = undefined
      patch.to = undefined
    }
    writeQuery(patch)
  }

  function setGrainOverride(next: Grain | null): void {
    writeQuery({ grain: next ?? undefined })
  }

  function setCompare(next: boolean): void {
    writeQuery({ compare: next ? 'prior' : undefined })
  }

  function setCustomRange(from: string, to: string): void {
    writeQuery({ range: 'custom', from, to })
  }

  return {
    preset,
    grain,
    compare,
    customFrom,
    customTo,
    window,
    priorWindow,
    setPreset,
    setGrainOverride,
    setCompare,
    setCustomRange,
  }
}
