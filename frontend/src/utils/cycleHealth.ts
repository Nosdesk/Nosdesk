/**
 * Cycle health: classifies an in-flight cycle as on-track / at-risk /
 * behind by comparing how much work is done against how much of the
 * scheduled window has elapsed (a pace check, not a deadline check).
 *
 * Derivable entirely from data we already have (ticket counts + the
 * cycle's start/end dates), so it works both in the burndown card (which
 * has real stats) and anywhere a per-cycle completed/total count is on
 * hand. No backend signal required.
 */
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
 * Slack between ideal and actual completion fraction that still counts as
 * on-pace. Below ideal but within this band reads as "at-risk"; further
 * behind reads as "behind".
 */
const AT_RISK_BAND = 0.15

export function cycleHealth(input: CycleHealthInput): CycleHealth {
  const { total, completed, startAt, endAt } = input
  const now = input.now ?? Date.now()

  if (total === 0) return 'not-started'
  if (completed >= total) return 'complete'

  // No end date means no schedule to fall behind against. Without a
  // deadline a cycle can't be "behind", so treat any in-progress work as
  // on-track until it's done.
  if (!endAt) return 'on-track'

  const end = new Date(endAt).getTime()
  const start = startAt ? new Date(startAt).getTime() : end - 14 * 86_400_000

  // Past the end date with work still open is unambiguously behind.
  if (now >= end) return 'behind'

  const span = end - start
  if (span <= 0) return 'on-track'

  const elapsedFraction = Math.min(1, Math.max(0, (now - start) / span))
  const doneFraction = completed / total
  const gap = elapsedFraction - doneFraction

  if (gap <= 0) return 'on-track'
  if (gap <= AT_RISK_BAND) return 'at-risk'
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
