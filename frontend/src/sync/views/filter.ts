/**
 * Client-side FilterState evaluator.
 *
 * Translates a FilterState into a `(card) => boolean` predicate
 * that the renderer applies before grouping and sorting. Filters
 * run client-side because:
 * - Sync groups already enforce authorization at the substrate
 *   level — a card the user shouldn't see is never in the pool.
 * - Re-running the predicate as the pool churns is cheap; sending
 *   filter changes round-trip to the server would block the UI.
 *
 * Phase 5 implements the operator subset that Triage and My Queue
 * actually need: `eq`, `in`, `is_empty`, `is_not_empty`. Adding an
 * operator is one match arm; the FilterState type already enumerates
 * the full set so future view shapes can extend without a wire
 * change.
 */
import type { CardData } from './types'
import type { FilterGroup, FilterPredicate, FilterState, QuickFilter } from './types'

/**
 * Build a card predicate from a FilterState. Composes:
 * - The structured predicate tree.
 * - Quick filters (mine, unassigned, etc.).
 * - The free-text search.
 * Returns a closure suitable for `Array.prototype.filter`.
 */
export function buildPredicate(
  filter: FilterState,
  context: { currentUserUuid: string | null },
): (card: CardData) => boolean {
  const tree = (card: CardData) => evalGroup(filter.predicate, card)
  const quick = quickFilterPredicate(filter.quick_filters, context)
  const text = textPredicate(filter.text_query)
  return (card) => tree(card) && quick(card) && text(card)
}

function evalGroup(group: FilterGroup, card: CardData): boolean {
  switch (group.combinator) {
    case 'AND':
      return group.children.every((child) =>
        isGroup(child) ? evalGroup(child, card) : evalPredicate(child, card),
      )
    case 'OR':
      return group.children.some((child) =>
        isGroup(child) ? evalGroup(child, card) : evalPredicate(child, card),
      )
    case 'NOT':
      return !group.children.some((child) =>
        isGroup(child) ? evalGroup(child, card) : evalPredicate(child, card),
      )
  }
}

function isGroup(child: FilterPredicate | FilterGroup): child is FilterGroup {
  return 'combinator' in child
}

function evalPredicate(p: FilterPredicate, card: CardData): boolean {
  const value = readField(p.field, card)
  switch (p.op) {
    case 'eq':
      return value === p.value
    case 'neq':
      return value !== p.value
    case 'in':
      return Array.isArray(p.value) && p.value.includes(value as never)
    case 'not_in':
      return Array.isArray(p.value) && !p.value.includes(value as never)
    case 'is_empty':
      return value == null || value === ''
    case 'is_not_empty':
      return value != null && value !== ''
    case 'has':
      return value != null
    case 'no':
      return value == null
    // Comparison operators (gt/lt/gte/lte/between) and the time-
    // window operator (changed_in_last) are spec'd in the
    // FilterState type but not yet implemented. Returning true is
    // permissive (rather than excluding cards entirely) so an
    // unfinished filter doesn't accidentally hide rows the user
    // expected to see; logging would be too chatty for a
    // legitimate caller waiting for the operator to ship.
    case 'gt':
    case 'lt':
    case 'gte':
    case 'lte':
    case 'between':
    case 'changed_in_last':
      return true
  }
}

/**
 * Field path resolution. Supports dotted paths like
 * `workflow_state.category` so the filter UI doesn't have to know
 * about the denormalised CardData shape.
 */
function readField(path: string, card: CardData): unknown {
  const parts = path.split('.')
  let cursor: unknown = card
  for (const part of parts) {
    if (cursor == null || typeof cursor !== 'object') return undefined
    cursor = (cursor as Record<string, unknown>)[part]
  }
  return cursor
}

function quickFilterPredicate(
  quickFilters: QuickFilter[],
  context: { currentUserUuid: string | null },
): (card: CardData) => boolean {
  if (quickFilters.length === 0) return () => true
  const checks: Array<(card: CardData) => boolean> = []
  for (const qf of quickFilters) {
    switch (qf) {
      case 'mine':
        checks.push((card) =>
          context.currentUserUuid != null &&
          card.assignee_uuid === context.currentUserUuid,
        )
        break
      case 'unassigned':
        checks.push((card) => card.assignee_uuid == null)
        break
      case 'overdue':
        checks.push((card) => {
          if (!card.due_date) return false
          return new Date(card.due_date).getTime() < Date.now()
        })
        break
      // The remaining quick filters depend on infrastructure we
      // haven't shipped (sla, mentions, kb-gap signal per ticket,
      // cycles). Until then they're no-ops; the FilterState type
      // still carries them so saved views referencing them
      // round-trip without loss.
      case 'sla_at_risk':
      case 'mentions_me':
      case 'starred':
      case 'has_kb_gap':
      case 'recently_updated':
      case 'in_my_cycles':
        break
    }
  }
  return (card) => checks.every((check) => check(card))
}

function textPredicate(query: string | undefined): (card: CardData) => boolean {
  if (!query || query.trim().length === 0) return () => true
  const lower = query.trim().toLowerCase()
  return (card) => card.title.toLowerCase().includes(lower)
}
