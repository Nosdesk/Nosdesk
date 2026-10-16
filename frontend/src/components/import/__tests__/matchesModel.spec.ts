import { describe, expect, it } from 'vitest'
import type { SavedMatch } from '@nosdesk/core/services/importService'

import { examples, firstName, isDiscard, sections } from '../matchesModel'

function entry(
  field: string,
  text: string,
  over: Partial<SavedMatch> = {},
): SavedMatch {
  return {
    id: `${field}:${over.context ?? ''}:${text}`,
    field,
    field_label: field,
    context: '',
    text,
    outcome: 'link',
    label: text.toUpperCase(),
    created: false,
    saved: { by: 'u1', by_name: 'Sam Okafor', at: '2026-07-12T00:00:00Z', job_id: 'j1' },
    ...over,
  }
}

// In field order, as the server lists them.
const ENTRIES: SavedMatch[] = [
  entry('kind', 'Cam'),
  entry('kind', 'B/cast'),
  entry('status', 'Spare'),
  entry('primary_user', 'nobody@example.com', { outcome: 'left_unassigned', label: undefined }),
  entry('manufacturer', 'Sony Corp'),
  entry('model', 'FX6', { context: 'sony', context_label: 'Sony' }),
  entry('model', 'URSA', { context: 'blackmagic', context_label: 'Blackmagic', created: true }),
  entry('model', 'C300', { context: '' }),
  entry('mount', 'ef', { context: 'camera', context_label: 'Camera' }),
]

describe('saved matches page model', () => {
  it('makes a section per field in order, with counts', () => {
    const got = sections(ENTRIES)
    expect(got.sections.map((s) => [s.field, s.total, s.flat])).toEqual([
      ['kind', 2, true],
      ['status', 1, true],
      ['primary_user', 1, true],
      ['manufacturer', 1, true],
      ['model', 3, false],
      ['mount', 1, false],
    ])
    expect(got.total).toBe(9)
    expect(got.shown).toBe(9)
  })

  it('gives a field answered in a context a sub-section per context', () => {
    const model = sections(ENTRIES).sections.find((s) => s.field === 'model')!
    expect(model.subs.map((s) => [s.context, s.label, s.entries.map((e) => e.text)])).toEqual([
      ['sony', 'Sony', ['FX6']],
      ['blackmagic', 'Blackmagic', ['URSA']],
      ['', null, ['C300']],
    ])
    const mount = sections(ENTRIES).sections.find((s) => s.field === 'mount')!
    expect(mount.subs.map((s) => s.label)).toEqual(['Camera'])
  })

  it('filters rows by search, hides what it empties and counts what is left', () => {
    const describe = (e: SavedMatch) => `${e.field} ${e.context_label ?? ''} ${e.label ?? ''}`
    const got = sections(ENTRIES, '  SONY ', describe)
    expect(got.sections.map((s) => [s.field, s.shown, s.total])).toEqual([
      ['manufacturer', 1, 1],
      ['model', 1, 3],
    ])
    expect(got.sections[1].subs.map((s) => s.context)).toEqual(['sony'])
    expect([got.shown, got.total]).toEqual([2, 9])

    const none = sections(ENTRIES, 'nothing like this', describe)
    expect(none.sections).toEqual([])
    expect([none.shown, none.total]).toEqual([0, 9])
  })

  it('searches what a row says, not only the spelling in the file', () => {
    const got = sections(ENTRIES, 'blackmagic', (e) => e.context_label ?? '')
    expect(got.sections.flatMap((s) => s.subs.flatMap((sub) => sub.entries.map((e) => e.text)))).toEqual([
      'URSA',
    ])
  })

  it('takes the hub examples from entries that keep data', () => {
    const shown = examples([ENTRIES[3], ...ENTRIES])
    expect(shown.map((e) => e.text)).toEqual(['Cam', 'B/cast', 'Spare'])
    expect(isDiscard(ENTRIES[3])).toBe(true)
    expect(isDiscard(ENTRIES[0])).toBe(false)
  })

  it('shortens a name to its first word', () => {
    expect(firstName('Sam Okafor')).toBe('Sam')
    expect(firstName('  Priya ')).toBe('Priya')
  })
})
