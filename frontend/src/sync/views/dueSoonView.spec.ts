import { describe, expect, it } from 'vitest'
import {
  BUILTIN_VIEWS,
  DUE_SOON_VIEW,
  OVERDUE_VIEW,
  findBuiltinView,
} from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import { sortCards } from '@/composables/useTicketsSort'

function card(id: number, category: WorkflowStateCategory, due_date: string | null): CardData {
  return {
    id,
    number: id,
    title: `Ticket ${id}`,
    workflow_state: { id, name: category, category, color: 'gray' },
    priority: 'none',
    due_date,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
  }
}

function dueSoon(cards: CardData[]): number[] {
  const keep = buildPredicate(DUE_SOON_VIEW.filter, { currentUserUuid: 'agent-uuid' })
  const sort = DUE_SOON_VIEW.shape.sort[0]
  return sortCards(cards.filter(keep), sort.field, sort.dir).map((c) => c.id)
}

describe('the Due soon view', () => {
  it('lists every open ticket, soonest due first, and the undated last', () => {
    const cards = [
      card(1, 'active', null),
      card(2, 'backlog', '2026-11-25T09:00:00Z'),
      card(3, 'triage', '2026-11-21T08:00:00+10:00'),
      card(4, 'in_review', '2026-11-20T23:00:00Z'),
      card(5, 'active', '2026-11-19T00:00:00'),
    ]
    // 5 is overdue (Nov 19), then 4 and 3 at Nov 20 22:00 and 23:00
    // UTC once 3's offset is applied, then 2; 1 has no date.
    expect(dueSoon(cards)).toEqual([5, 3, 4, 2, 1])
  })

  it('leaves out done, cancelled and merged tickets, due date or not', () => {
    const cards = [
      card(1, 'done', '2026-11-20T00:00:00Z'),
      card(2, 'cancelled', '2026-11-20T00:00:00Z'),
      card(3, 'merged', null),
      card(4, 'active', '2026-11-22T00:00:00Z'),
    ]
    expect(dueSoon(cards)).toEqual([4])
  })

  it('shows the Due column in place of Updated', () => {
    const fields = DUE_SOON_VIEW.shape.visible_card_fields
    expect(fields).toContain('due_date')
    expect(fields).not.toContain('last_activity_at')
    const columns = 'columns' in DUE_SOON_VIEW.shape ? DUE_SOON_VIEW.shape.columns : []
    expect(columns.map((c) => c.field)).toContain('due_date')
    expect(columns.map((c) => c.field)).not.toContain('last_activity_at')
  })

  it('sits after Overdue in the view list and resolves from the URL', () => {
    const order = BUILTIN_VIEWS.map((v) => v.id)
    expect(order.indexOf('due-soon')).toBe(order.indexOf(OVERDUE_VIEW.id) + 1)
    expect(findBuiltinView('due-soon')).toBe(DUE_SOON_VIEW)
  })
})
