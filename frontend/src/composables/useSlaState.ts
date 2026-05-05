/**
 * Compute the visual SLA state for a card. Centralises the tone
 * class / label / time-bar fraction logic that previously lived
 * inline in TicketsTable (compact pill text + tone) and
 * TicketPreviewPane (full bar with detail line).
 *
 * Returns null when the card has no SLA — consumers conditionally
 * render the SLA chrome only when this resolves.
 *
 * The bar fraction is best-effort: we don't have `started_at`
 * (only `target_at` and `seconds_remaining`), so the fill is
 * mapped from time-remaining-buckets. It reads as an urgency
 * indicator rather than a literal progress bar, which is
 * consistent with how Linear / Plain present SLA pills.
 */
import { computed, type ComputedRef, type Ref } from 'vue'
import type { CardData } from '@/sync/views/types'

export interface SlaState {
  /** Compact pill text — used in the table column. */
  compactLabel: string
  /** "Breached" / "At risk" / "On track" / "Paused" — used as the
   * leading word in the preview pane's SLA section. */
  statusLabel: string
  /** Tone class for any text rendered in this state. */
  toneClass: string
  /** Tone class for the bar fill in the preview pane. */
  barClass: string
  /** 0..1 fill fraction for the urgency bar. */
  fraction: number
  /** Detail line for the preview pane, eg.
   * "3 hours remaining · target Tue 12:00 PM". */
  detail: string
  /** Formatted target timestamp (full, eg. "Tue, May 7 12:00 PM"). */
  target: string
  /** True when this state should treat the SLA as broken. */
  breached: boolean
  /** True when the timer is paused. */
  paused: boolean
}

function fullDateTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

function compactRemaining(seconds: number): string {
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`
  if (seconds < 86_400) return `${Math.ceil(seconds / 3600)}h`
  return `${Math.ceil(seconds / 86_400)}d`
}

function detailRemaining(seconds: number): string {
  if (seconds < 3600) return `${Math.ceil(seconds / 60)} min remaining`
  if (seconds < 86_400) return `${Math.ceil(seconds / 3600)} hours remaining`
  return `${Math.ceil(seconds / 86_400)} days remaining`
}

/**
 * Map remaining time to a fill fraction. 24h+ remaining = 25%
 * filled (mostly empty bar = lots of time). Less than 1h = 85%
 * (almost full = urgent). The exact mapping is pedagogical —
 * meant to communicate urgency at a glance, not measure SLA
 * progress precisely.
 */
function urgencyFraction(seconds: number): number {
  if (seconds > 86_400) return 0.25
  if (seconds > 14_400) return 0.45
  if (seconds > 3600) return 0.65
  return 0.85
}

export function deriveSlaState(card: CardData | null): SlaState | null {
  if (!card?.sla) return null
  const sla = card.sla
  const target = fullDateTime(sla.target_at)

  if (sla.breached) {
    return {
      compactLabel: 'Breached',
      statusLabel: 'Breached',
      toneClass: 'text-rose-600 dark:text-rose-400',
      barClass: 'bg-rose-500',
      fraction: 1,
      detail: `Past target · ${target}`,
      target,
      breached: true,
      paused: false,
    }
  }

  if (sla.paused) {
    return {
      compactLabel: 'Paused',
      statusLabel: 'Paused',
      toneClass: 'text-zinc-500 dark:text-zinc-400',
      barClass: 'bg-zinc-400',
      fraction: 0.5,
      detail: `Target ${target}`,
      target,
      breached: false,
      paused: true,
    }
  }

  const remaining = sla.seconds_remaining ?? 0
  const compact = compactRemaining(remaining)
  const detail = `${detailRemaining(remaining)} · target ${target}`
  const fraction = urgencyFraction(remaining)

  if (sla.pill_color === 'amber') {
    return {
      compactLabel: compact,
      statusLabel: 'At risk',
      toneClass: 'text-amber-600 dark:text-amber-400',
      barClass: 'bg-amber-500',
      fraction,
      detail,
      target,
      breached: false,
      paused: false,
    }
  }

  return {
    compactLabel: compact,
    statusLabel: 'On track',
    toneClass: 'text-emerald-600 dark:text-emerald-400',
    barClass: 'bg-emerald-500',
    fraction,
    detail,
    target,
    breached: false,
    paused: false,
  }
}

/** Reactive form: pass a ref / getter, get a computed SlaState
 * that re-evaluates when the card changes. */
export function useSlaState(
  card: Ref<CardData | null> | (() => CardData | null),
): ComputedRef<SlaState | null> {
  return computed(() => {
    const c = typeof card === 'function' ? card() : card.value
    return deriveSlaState(c)
  })
}
