/**
 * Shared priority-related rendering helpers.
 *
 * The same priority-to-display logic was duplicated across the
 * tickets table and the preview pane, plus a couple of SLA tone
 * variants. Centralising it here means the `urgent` colour stays
 * consistent in every surface and a future palette change is one
 * file edit.
 */
import { translate } from '@/i18n'
import type { CardData, Priority } from '@/sync/views/types'

/** PriorityIndicator only knows about three levels; collapse
 * `urgent` to `high` for that visualization, and `none` to null. */
export function priorityForBadge(p: Priority): 'low' | 'medium' | 'high' | null {
  if (p === 'urgent') return 'high'
  if (p === 'low' || p === 'medium' || p === 'high') return p
  return null
}

export function priorityLabel(p: Priority): string {
  if (p === 'urgent') return translate('priority-urgent', undefined, 'Urgent')
  if (p === 'high') return translate('priority-high', undefined, 'High')
  if (p === 'medium') return translate('priority-medium', undefined, 'Medium')
  if (p === 'low') return translate('priority-low', undefined, 'Low')
  return translate('priority-none', undefined, 'No priority')
}

/** Subtle inline tint used in the title cell to make urgent /
 * high tickets visually pop in the table. Returns null for
 * everything else (no tint applied). */
export function inlinePriorityClass(p: Priority): string | null {
  if (p === 'urgent') return 'text-rose-500'
  if (p === 'high') return 'text-orange-500'
  return null
}

/** Pill tone class for the dedicated Priority pill in the preview
 * pane. Heavier styling than the inline tint — meant to read as
 * "this whole pill is the priority indicator." */
export function priorityToneClass(p: Priority): string {
  if (p === 'urgent') return 'text-rose-600 dark:text-rose-400'
  if (p === 'high') return 'text-orange-600 dark:text-orange-400'
  return 'text-secondary'
}

/** Leading row stripe encoding SLA urgency, used by the table.
 * Returns the bg class for the 3px strip — empty string when
 * the row has no SLA tone to communicate so we don't burn
 * visual budget on a transparent strip. */
export function rowSlaToneClass(card: CardData): string {
  const sla = card.sla
  if (!sla) return ''
  if (sla.breached) return 'bg-rose-500'
  if (sla.pill_color === 'amber') return 'bg-amber-500'
  return ''
}
