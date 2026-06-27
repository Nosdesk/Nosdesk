/**
 * Cycle health: classifies an in-flight cycle as on-track / at-risk /
 * behind from a FORECAST, not from distance to a straight ideal line.
 *
 * Per the agile-metrics research (see docs/plans/gantt-cycle-design-
 * overhaul.md), "distance from the day-zero ideal" is the rejected
 * approach: it assumes linear work and punishes a team that simply
 * front-loads discovery. Instead we project the team's observed
 * throughput (completed so far / working days elapsed) forward and ask
 * whether the projected finish lands before the cycle end. This is the
 * count-based analogue of Cohn's velocity forecast.
 *
 * Inputs are intentionally the same coarse counts + dates the burndown
 * card already has, so callers don't need the daily series to get a
 * health read.
 */
import { countWorkingDays, dayMsOf } from '@nosdesk/core/utils/burnupModel'
import type { StatusPillTone } from '@/components/common/statusPillTone'

export type CycleHealth =
  | 'on-track'
  | 'at-risk'
  | 'behind'
  | 'complete'
  | 'not-started'

export interface CycleHealthInput {
  total: number
  completed: number
  startAt: string | null
  endAt: string | null
  /** Injectable for tests; defaults to now. */
  now?: number
}

/**
 * Slack, as a fraction of the cycle's total working days, that still
 * counts as on-pace. A projected finish later than the end but within
 * this band reads as "at-risk"; further past reads as "behind".
 */
const AT_RISK_BAND = 0.2

/**
 * Too little of the cycle elapsed to forecast meaningfully: below this
 * fraction we give the benefit of the doubt rather than crying wolf on
 * day one (the empty-data caveat from the ideal-line critique).
 */
const MIN_ELAPSED_FRACTION = 0.15

export function cycleHealth(input: CycleHealthInput): CycleHealth {
  const { total, completed, startAt, endAt } = input
  const now = input.now ?? Date.now()

  if (total === 0) return 'not-started'
  if (completed >= total) return 'complete'

  // No end date means no schedule to fall behind against.
  if (!endAt) return 'on-track'

  const end = dayMsOf(new Date(endAt).getTime())
  const start = dayMsOf(startAt ? new Date(startAt).getTime() : end - 14 * 86_400_000)
  const today = dayMsOf(now)

  // Past the end date with work still open is unambiguously behind.
  if (today >= end) return 'behind'

  const totalWorking = countWorkingDays(start, end)
  if (totalWorking <= 0) return 'on-track'

  const elapsedWorking = countWorkingDays(start, today)
  const elapsedFraction = elapsedWorking / totalWorking

  // Too early to judge: don't flag risk before there's signal.
  if (elapsedFraction < MIN_ELAPSED_FRACTION) return 'on-track'

  // Nothing done well into the cycle is the clearest behind signal.
  if (completed === 0) return 'behind'

  // Project throughput forward: working days the remaining work needs at
  // the observed rate, added to what's already elapsed, versus the
  // cycle's working-day budget.
  const rate = completed / Math.max(1, elapsedWorking)
  const remaining = total - completed
  const neededWorking = elapsedWorking + remaining / rate
  const slackFraction = (totalWorking - neededWorking) / totalWorking

  if (slackFraction >= 0) return 'on-track'
  if (slackFraction >= -AT_RISK_BAND) return 'at-risk'
  return 'behind'
}

/** Pill tone + i18n label key for a given health classification. */
export function cycleHealthPresentation(
  health: CycleHealth,
): { tone: StatusPillTone; labelKey: string } {
  switch (health) {
    case 'on-track':
      return { tone: 'positive', labelKey: 'project-cycles-health-on-track' }
    case 'at-risk':
      return { tone: 'caution', labelKey: 'project-cycles-health-at-risk' }
    case 'behind':
      return { tone: 'critical', labelKey: 'project-cycles-health-behind' }
    case 'complete':
      return { tone: 'positive', labelKey: 'project-cycles-health-complete' }
    default:
      return { tone: 'neutral', labelKey: 'project-cycles-health-not-started' }
  }
}
