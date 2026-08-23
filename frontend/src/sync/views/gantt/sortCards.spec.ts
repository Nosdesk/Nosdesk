import { describe, it, expect } from 'vitest'
import { sortCards } from './sortCards'
import type { CardData, Priority } from '@nosdesk/core/sync/views/types'

let nextId = 1

function card(over: {
  priority?: Priority
  start_date?: string | null
  due_date?: string | null
  created_at?: string
}): CardData {
  return {
    id: nextId++,
    title: `t${nextId}`,
    workflow_state: { id: 1, name: 'In Progress', category: 'active', color: '#000' },
    priority: over.priority ?? 'none',
    start_date: over.start_date ?? null,
    due_date: over.due_date ?? null,
    created_at: over.created_at ?? '2026-08-01',
    updated_at: '2026-08-01',
    last_activity_at: '2026-08-01',
  } as CardData
}

const ids = (cards: CardData[]) => cards.map((c) => c.id)

describe('sortCards', () => {
  it('start: orders by the drawn left edge, start_date falling back to created_at', () => {
    const late = card({ start_date: '2026-08-20', due_date: '2026-08-25' })
    const early = card({ start_date: '2026-08-05', due_date: '2026-08-25' })
    // No planning start: the bar starts at created_at, so the sort uses it too.
    const byCreation = card({ created_at: '2026-08-10', due_date: '2026-08-25' })
    expect(ids(sortCards([late, byCreation, early], 'start'))).toEqual([
      early.id,
      byCreation.id,
      late.id,
    ])
  })

  it('due: orders by due date with unscheduled (tray) cards last', () => {
    const soon = card({ due_date: '2026-08-10' })
    const later = card({ due_date: '2026-08-30' })
    const tray = card({ due_date: null })
    expect(ids(sortCards([tray, later, soon], 'due'))).toEqual([soon.id, later.id, tray.id])
  })

  it('priority: urgent first, none last', () => {
    const low = card({ priority: 'low', due_date: '2026-08-10' })
    const urgent = card({ priority: 'urgent', due_date: '2026-08-10' })
    const unset = card({ priority: 'none', due_date: '2026-08-10' })
    const high = card({ priority: 'high', due_date: '2026-08-10' })
    expect(ids(sortCards([low, unset, urgent, high], 'priority'))).toEqual([
      urgent.id,
      high.id,
      low.id,
      unset.id,
    ])
  })

  it('is stable: ties keep the shared project order', () => {
    const a = card({ priority: 'high', due_date: '2026-08-10' })
    const b = card({ priority: 'high', due_date: '2026-08-10' })
    const c = card({ priority: 'high', due_date: '2026-08-10' })
    expect(ids(sortCards([a, b, c], 'priority'))).toEqual([a.id, b.id, c.id])
    expect(ids(sortCards([c, a, b], 'priority'))).toEqual([c.id, a.id, b.id])
  })
})
