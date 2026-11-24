import { describe, expect, it } from 'vitest'
import {
  BUILTIN_VIEWS,
  MY_DAY_VIEW,
  MY_OPEN_VIEW,
  findBuiltinView,
} from '@nosdesk/core/sync/views/builtinViews'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import { planFor } from '@nosdesk/core/sync/views/planOrder'
import type { CardData } from '@nosdesk/core/sync/views/types'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const ME = 'agent-uuid'

function card(
  id: number,
  category: WorkflowStateCategory,
  due_date: string | null,
  assignee_uuid: string | null = ME,
  plan: Partial<CardData> = {},
): CardData {
  return {
    ...plan,
    id,
    number: id,
    title: `Ticket ${id}`,
    workflow_state: { id, name: category, category, color: 'gray' },
    priority: 'none',
    assignee_uuid,
    due_date,
    created_at: `2026-11-0${id}T00:00:00Z`,
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
  }
}

const DAY = '2026-11-21'

/** The day's jobs as the agenda shows them: Booked, then Any time. */
function myDay(cards: CardData[], day = DAY): number[] {
  const keep = buildPredicate(MY_DAY_VIEW.filter, { currentUserUuid: ME })
  const plan = planFor(cards.filter(keep), day)
  return [...plan.booked, ...plan.anyTime].map((c) => c.id)
}

describe('the My day view', () => {
  it('shows the jobs planned for the day: booked by time, then the rest', () => {
    const cards = [
      card(1, 'active', null),
      card(2, 'backlog', '2026-11-25T00:00:00Z'),
      card(3, 'triage', '2026-11-21T00:00:00+11:00'),
      card(4, 'in_review', '2026-11-21T00:00:00', ME, { due_time: '14:30:00' }),
      card(5, 'active', '2026-11-28T00:00:00', ME, { start_date: '2026-11-21T00:00:00', start_time: '09:00:00' }),
      card(6, 'active', '2026-11-21T00:00:00', ME, { start_date: '2026-11-24T00:00:00' }),
    ]
    expect(myDay(cards)).toEqual([5, 4, 3])
    expect(myDay(cards, '2026-11-24')).toEqual([6])
  })

  it("leaves out other people's tickets, unassigned ones, and closed ones", () => {
    const cards = [
      card(1, 'active', '2026-11-21T00:00:00', 'someone-else'),
      card(2, 'active', '2026-11-21T00:00:00', null),
      card(3, 'done', '2026-11-21T00:00:00'),
      card(4, 'cancelled', null),
      card(5, 'merged', null),
      card(6, 'active', '2026-11-21T00:00:00'),
    ]
    expect(myDay(cards)).toEqual([6])
  })

  it('is an agenda that keeps a list layout, Due in place of Updated, for its fall-through', () => {
    expect(MY_DAY_VIEW.shape.type).toBe('agenda')
    const fields = MY_DAY_VIEW.shape.visible_card_fields
    expect(fields).toContain('due_date')
    expect(fields).not.toContain('last_activity_at')
  })

  it('is second in the view list, after My Open, and resolves from the URL', () => {
    expect(BUILTIN_VIEWS.slice(0, 2).map((v) => v.id)).toEqual([MY_OPEN_VIEW.id, 'my-day'])
    expect(findBuiltinView('my-day')).toBe(MY_DAY_VIEW)
    expect(findBuiltinView('due-soon')).toBeNull()
  })

  it('takes the place of My Active, whose links open My day', () => {
    expect(BUILTIN_VIEWS.map((v) => v.id)).not.toContain('my-active')
    expect(findBuiltinView('my-active')).toBe(MY_DAY_VIEW)
  })
})
