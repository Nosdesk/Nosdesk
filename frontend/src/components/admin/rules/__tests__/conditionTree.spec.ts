import { describe, expect, it } from 'vitest'
import type { RuleCondition } from '@nosdesk/core/types/rule'
import {
  CONDITION_FIELDS,
  addChild,
  emptyGroup,
  fieldsFor,
  fromLocalInput,
  fromWire,
  leafProblem,
  newLeaf,
  opsFor,
  removeNode,
  setMode,
  toLocalInput,
  toWire,
  treeComplete,
  updateLeaf,
} from '../conditionTree'
import { readTriggerConfig, triggerConfigFor } from '../triggerConfig'

const leaf = (field: string, op: string, value: unknown): RuleCondition =>
  ({ kind: 'leaf', field, op, value }) as RuleCondition

describe('condition tree wire shape', () => {
  it('reads and writes no conditions as []', () => {
    expect(fromWire([])).toEqual(emptyGroup())
    expect(toWire(fromWire([]))).toEqual([])
  })

  it('keeps and, or and not groups, nested, as they were', () => {
    const tree: RuleCondition = {
      kind: 'and',
      children: [
        leaf('ticket.priority', 'eq', 'high'),
        {
          kind: 'or',
          children: [
            leaf('ticket.tag_ids', 'has_any', [5]),
            { kind: 'not', child: { kind: 'or', children: [leaf('ticket.title', 'contains', 'printer')] } },
            { kind: 'not', child: { kind: 'and', children: [leaf('ticket.assignee_uuid', 'is_null', null)] } },
          ],
        },
      ],
    }
    const root = fromWire(tree)
    expect(root.mode).toBe('all')
    const nested = root.children[1]
    expect(nested.kind === 'group' && nested.mode).toBe('any')
    expect(nested.kind === 'group' && nested.children.map((c) => (c.kind === 'group' ? c.mode : 'leaf'))).toEqual([
      'leaf',
      'none',
      'not_all',
    ])
    expect(toWire(root)).toEqual(tree)
  })

  it('reads a bare leaf or a not around a leaf as an equivalent group', () => {
    expect(toWire(fromWire(leaf('ticket.priority', 'eq', 'high')))).toEqual({
      kind: 'and',
      children: [leaf('ticket.priority', 'eq', 'high')],
    })
    expect(toWire(fromWire({ kind: 'not', child: leaf('ticket.priority', 'eq', 'high') }))).toEqual({
      kind: 'not',
      child: { kind: 'or', children: [leaf('ticket.priority', 'eq', 'high')] },
    })
  })

  it('sends no value for the empty checks', () => {
    const root = addChild(emptyGroup(), [], { kind: 'leaf', field: 'ticket.assignee_uuid', op: 'is_null', value: 'stale' })
    expect(toWire(root)).toEqual({ kind: 'and', children: [leaf('ticket.assignee_uuid', 'is_null', null)] })
  })
})

describe('condition tree edits', () => {
  it('adds and removes conditions and groups at any depth, leaving the old tree as it was', () => {
    let root = addChild(emptyGroup(), [], newLeaf('ticket.priority'))
    const group = emptyGroup('any')
    root = addChild(root, [], group)
    root = addChild(root, [1], newLeaf('ticket.title'))
    root = addChild(root, [1], emptyGroup('none'))
    root = addChild(root, [1, 1], newLeaf('ticket.tag_ids'))
    const before = structuredClone(root)

    const removed = removeNode(root, [1, 0])
    expect(root).toEqual(before)
    const nested = removed.children[1]
    expect(nested.kind === 'group' && nested.children).toHaveLength(1)
    expect(nested.kind === 'group' && nested.children[0].kind).toBe('group')

    expect(removeNode(root, [1]).children).toHaveLength(1)
    expect(removeNode(root, []).children).toEqual([])
    expect(setMode(root, [1, 1], 'all').children[1]).toMatchObject({ children: [{}, { mode: 'all' }] })
  })

  it('keeps the operator and value a new field still takes, and resets what it does not', () => {
    let root = addChild(emptyGroup(), [], { kind: 'leaf', field: 'ticket.title', op: 'contains', value: 'vpn' })
    // Another text field takes contains and the text.
    root = updateLeaf(root, [0], { field: 'ticket.requester.email' })
    expect(root.children[0]).toEqual({ kind: 'leaf', field: 'ticket.requester.email', op: 'contains', value: 'vpn' })
    // Priority doesn't take contains: it starts on eq with no value.
    root = updateLeaf(root, [0], { field: 'ticket.priority' })
    expect(root.children[0]).toEqual({ kind: 'leaf', field: 'ticket.priority', op: 'eq', value: '' })
    root = updateLeaf(root, [0], { value: 'high' })
    // eq to in turns the value into a list.
    root = updateLeaf(root, [0], { op: 'in' })
    expect(root.children[0]).toMatchObject({ op: 'in', value: [] })
    root = updateLeaf(root, [0], { op: 'not_in', value: ['low'] })
    expect(root.children[0]).toMatchObject({ op: 'not_in', value: ['low'] })
  })
})

describe('fields and operators', () => {
  // The server's operator matrix (ConditionFieldKind::allowed_ops), less the
  // empty checks on fields that are never empty.
  const expected: Record<string, string[]> = {
    'ticket.title': ['eq', 'neq', 'contains', 'not_contains', 'starts_with', 'ends_with', 'in', 'not_in', 'matches_regex'],
    'ticket.requester.email': [
      'eq',
      'neq',
      'contains',
      'not_contains',
      'starts_with',
      'ends_with',
      'in',
      'not_in',
      'matches_regex',
      'is_null',
      'is_not_null',
    ],
    'ticket.priority': ['eq', 'neq', 'in', 'not_in'],
    'ticket.workflow_state_id': ['eq', 'neq', 'in', 'not_in'],
    'ticket.category_id': ['eq', 'neq', 'in', 'not_in', 'is_null', 'is_not_null'],
    'ticket.assignee_uuid': ['eq', 'neq', 'in', 'not_in', 'is_null', 'is_not_null'],
    'ticket.tag_ids': ['has_any', 'has_all', 'is_null', 'is_not_null'],
    'event.changed_fields': ['has_any', 'has_all', 'is_null', 'is_not_null'],
    'ticket.created_at': ['gt', 'lt', 'gte', 'lte'],
    'ticket.resolved_at': ['gt', 'lt', 'gte', 'lte', 'is_null', 'is_not_null'],
    'reply.kind': ['eq', 'neq', 'in', 'not_in'],
    'clock.minutes_since': ['gt', 'lt', 'gte', 'lte'],
  }

  it.each(Object.entries(expected))('%s takes the operators the server accepts', (field, ops) => {
    expect(opsFor(field)).toEqual(ops)
  })

  it('offers event fields only to the trigger that carries them', () => {
    const names = (trigger: Parameters<typeof fieldsFor>[0]) => fieldsFor(trigger).map((f) => f.name)
    expect(names('ticket_created')).not.toContain('event.changed_fields')
    expect(names('ticket_created')).not.toContain('reply.kind')
    expect(names('ticket_updated')).toContain('event.changed_fields')
    expect(names('ticket_replied')).toEqual(expect.arrayContaining(['reply.kind', 'reply.author_role']))
    expect(names('ticket_replied')).not.toContain('event.changed_fields')
  })

  it('leaves out the fields the server cannot read yet', () => {
    const names = CONDITION_FIELDS.map((f) => f.name)
    for (const gone of ['ticket.status', 'ticket.body', 'ticket.source_channel_id', 'ticket.organization_id']) {
      expect(names).not.toContain(gone)
    }
  })
})

describe('what a condition still needs', () => {
  const check = (field: string, op: string, value: unknown, trigger = 'ticket_created') =>
    leafProblem({ kind: 'leaf', field, op, value } as never, trigger as never)

  it('wants a value for every operator that takes one', () => {
    expect(check('ticket.title', 'contains', '  ')).toBe('admin-rule-conditions-problem-value')
    expect(check('ticket.title', 'contains', 'vpn')).toBeNull()
    expect(check('ticket.priority', 'in', [])).toBe('admin-rule-conditions-problem-value')
    expect(check('ticket.workflow_state_id', 'eq', null)).toBe('admin-rule-conditions-problem-value')
    expect(check('ticket.workflow_state_id', 'eq', 4)).toBeNull()
    expect(check('ticket.created_at', 'gt', 'not a date')).toBe('admin-rule-conditions-problem-value')
    expect(check('ticket.created_at', 'gt', '2026-01-31T09:00:00.000Z')).toBeNull()
    expect(check('ticket.assignee_uuid', 'is_null', null)).toBeNull()
  })

  it('flags a field from another trigger, and an operator the field does not take', () => {
    expect(check('reply.kind', 'eq', 'agent')).toBe('admin-rule-conditions-problem-trigger')
    expect(check('reply.kind', 'eq', 'agent', 'ticket_replied')).toBeNull()
    expect(check('ticket.priority', 'contains', 'hi')).toBe('admin-rule-conditions-problem-op')
  })

  it('counts an empty nested group as unfinished, but not an empty root', () => {
    expect(treeComplete(emptyGroup(), 'ticket_created')).toBe(true)
    const root = addChild(emptyGroup(), [], emptyGroup('any'))
    expect(treeComplete(root, 'ticket_created')).toBe(false)
  })
})

describe('date and time values', () => {
  it('round trips through the datetime-local input', () => {
    const iso = fromLocalInput('2026-03-04T05:06')
    expect(Number.isNaN(Date.parse(iso))).toBe(false)
    expect(toLocalInput(iso)).toBe('2026-03-04T05:06')
    expect(fromLocalInput('')).toBe('')
    expect(toLocalInput(null)).toBe('')
  })
})

describe('trigger settings', () => {
  it('round trips the changed fields of an updated trigger and the reply kind of a replied one', () => {
    const updated = { changed_fields_include: ['ticket.priority', 'ticket.tag_ids'] }
    expect(triggerConfigFor('ticket_updated', readTriggerConfig(updated))).toEqual(updated)
    expect(triggerConfigFor('ticket_updated', readTriggerConfig({}))).toEqual({})
    expect(triggerConfigFor('ticket_replied', readTriggerConfig({ reply_kind: 'agent' }))).toEqual({ reply_kind: 'agent' })
    // A replied rule always names whose replies run it.
    expect(triggerConfigFor('ticket_replied', readTriggerConfig({}))).toEqual({ reply_kind: 'customer' })
    // Other triggers take nothing, whatever the form holds.
    expect(triggerConfigFor('ticket_created', readTriggerConfig(updated))).toEqual({})
    expect(triggerConfigFor('manual', readTriggerConfig({ reply_kind: 'any' }))).toEqual({})
  })
})
