import { describe, expect, it } from 'vitest'
import {
  ALL_ACTIVE_VIEW,
  BUILTIN_VIEWS,
  DASHBOARD_BUILTIN_VIEWS,
  MY_DAY_VIEW,
  MY_OPEN_VIEW,
  isDashboardBuiltinViewId,
  resolveDashboardViewFilter,
  type BuiltInView,
} from '@nosdesk/core/sync/views/builtinViews'
import { fallsThroughToAllActive } from '@nosdesk/core/sync/views/fallThrough'
import { buildPredicate } from '@nosdesk/core/sync/views/filter'
import type { CardData, FilterState } from '@nosdesk/core/sync/views/types'
import { TERMINAL_CATEGORIES, type WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const ME = 'agent-uuid'
const CTX = { currentUserUuid: ME }

const OPEN_CATEGORIES: WorkflowStateCategory[] = ['triage', 'backlog', 'active', 'in_review']
const CLOSED: WorkflowStateCategory[] = [...TERMINAL_CATEGORIES]

let nextId = 1

/** A ticket as the pool holds it: a closed one carries `closed_at`, which
 * the database sets on reaching a terminal category. */
function card(
  category: WorkflowStateCategory,
  assignee_uuid: string | null = ME,
  due_date: string | null = null,
): CardData {
  const id = nextId++
  return {
    id,
    number: id,
    title: `Ticket ${id}`,
    workflow_state: { id, name: category, category, color: 'gray' },
    priority: 'none',
    assignee_uuid,
    due_date,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
    closed_at: CLOSED.includes(category) ? '2026-11-02T00:00:00Z' : null,
  }
}

const keep = (filter: FilterState) => buildPredicate(filter, CTX)

describe('the My Open view', () => {
  it('lists your tickets in every open category', () => {
    const cards = OPEN_CATEGORIES.map((c) => card(c))
    expect(cards.filter(keep(MY_OPEN_VIEW.filter))).toEqual(cards)
  })

  it('never lists a ticket in a closed category, even one assigned to you', () => {
    const open = card('active')
    const cards = [...CLOSED.map((c) => card(c)), open, card('active', 'someone-else'), card('triage', null)]
    expect(cards.filter(keep(MY_OPEN_VIEW.filter))).toEqual([open])
  })
})

describe('the landing views falling through to All Active', () => {
  const myOpen = keep(MY_OPEN_VIEW.filter)
  const allActive = keep(ALL_ACTIVE_VIEW.filter)

  // The production report: an owner with plenty of Done work assigned and
  // open, unassigned, emailed tickets waiting.
  const done = Array.from({ length: 30 }, () => card('done'))
  const waiting = Array.from({ length: 16 }, () => card('triage', null))
  const pool = [...done, ...waiting]

  it('show All Active when only closed tickets are assigned to you', () => {
    expect(pool.filter(myOpen)).toEqual([])
    for (const id of [MY_OPEN_VIEW.id, MY_DAY_VIEW.id]) {
      expect(fallsThroughToAllActive(id, pool, myOpen, allActive)).toBe(true)
    }
    expect(pool.filter(allActive)).toEqual(waiting)
  })

  it('stay put while one open ticket is assigned to you', () => {
    const cards = [...pool, card('active')]
    expect(fallsThroughToAllActive(MY_OPEN_VIEW.id, cards, myOpen, allActive)).toBe(false)
  })

  it('stay put, empty, when nothing anywhere is open', () => {
    expect(fallsThroughToAllActive(MY_OPEN_VIEW.id, done, myOpen, allActive)).toBe(false)
  })

  it('only apply to My Open and My day', () => {
    for (const id of ['all-active', 'all-tickets', 'unassigned', 'triage', 'calendar']) {
      expect(fallsThroughToAllActive(id, pool, myOpen, allActive)).toBe(false)
    }
  })
})

describe('built-in views that promise open work', () => {
  /** The filter the tickets list applies for the view. */
  function effectiveFilter(view: BuiltInView): FilterState {
    return isDashboardBuiltinViewId(view.id) ? resolveDashboardViewFilter(view.id) : view.filter
  }

  const promisesOpen = (v: BuiltInView) => /\b(open|active)\b/i.test(`${v.name} ${v.description}`)
  const views = [...BUILTIN_VIEWS, ...DASHBOARD_BUILTIN_VIEWS].filter(promisesOpen)

  // Pinned so a reworded description can't quietly drop a view from the check.
  it('are the ones this check covers', () => {
    expect(views.map((v) => v.id).sort()).toEqual(
      ['all-active', 'dashboard-open', 'my-day', 'my-open', 'overdue', 'unassigned'],
    )
  })

  // Shapes that between them satisfy every quick filter these views use:
  // mine, unassigned and overdue.
  const variants: Array<[string | null, string | null]> = [
    [ME, null],
    [ME, '2020-01-01T00:00:00'],
    [null, null],
    [null, '2020-01-01T00:00:00'],
  ]

  it.each(views.map((v) => [v.id, v] as const))('%s leaves out every closed category', (_id, view) => {
    const predicate = keep(effectiveFilter(view))
    for (const [assignee, due] of variants) {
      for (const category of CLOSED) {
        expect(predicate(card(category, assignee, due)), `${category} ${assignee} ${due}`).toBe(false)
      }
    }
    // The same shapes, open, do get in: the check isn't passing on an
    // impossible fixture.
    expect(variants.some(([assignee, due]) => predicate(card('active', assignee, due)))).toBe(true)
  })
})
