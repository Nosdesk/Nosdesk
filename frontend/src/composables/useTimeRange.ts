/**
 * URL-bound time range for the dashboard surface
 * (docs/dashboard-and-analytics-plan.md decision 5).
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

/** The six preset chips + the Custom escape hatch. */
export type TimeRangePreset =
  | 'today'
  | '7d'
  | '30d'
  | '90d'
  | 'quarter'
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
    case 'quarter':
      return 'week'
    case 'custom':
      // Custom defaults to day; the picker can override.
      return 'day'
  }
}

/** Compute the absolute (from, to) window for a preset. */
export function presetWindow(
  preset: TimeRangePreset,
  custom?: { from?: string; to?: string },
): { from: Date; to: Date } {
  const now = new Date()
  const to = new Date(now)
  const from = new Date(now)
  switch (preset) {
    case 'today':
      from.setHours(0, 0, 0, 0)
      return { from, to }
    case '7d':
      from.setDate(from.getDate() - 7)
      return { from, to }
    case '30d':
      from.setDate(from.getDate() - 30)
      return { from, to }
    case '90d':
      from.setDate(from.getDate() - 90)
      return { from, to }
    case 'quarter': {
      const quarter = Math.floor(now.getMonth() / 3)
      const start = new Date(now.getFullYear(), quarter * 3, 1)
      return { from: start, to }
    }
    case 'custom': {
      const f = custom?.from ? new Date(custom.from) : from
      const t = custom?.to ? new Date(custom.to) : to
      return { from: f, to: t }
    }
  }
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
    case 'quarter':
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
export interface TimeRangeHandle {
  preset: ComputedRef<TimeRangePreset>
  grain: ComputedRef<Grain>
  compare: ComputedRef<boolean>
  customFrom: ComputedRef<string | null>
  customTo: ComputedRef<string | null>
  window: ComputedRef<{ from: Date; to: Date }>
  setPreset: (next: TimeRangePreset) => void
  setGrainOverride: (next: Grain | null) => void
  setCompare: (next: boolean) => void
  setCustomRange: (from: string, to: string) => void
}

export function useTimeRange(): TimeRangeHandle {
  const route = useRoute()
  const router = useRouter()

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

  const window = computed<{ from: Date; to: Date }>(() =>
    presetWindow(preset.value, {
      from: customFrom.value ?? undefined,
      to: customTo.value ?? undefined,
    }),
  )

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
    setPreset,
    setGrainOverride,
    setCompare,
    setCustomRange,
  }
}
