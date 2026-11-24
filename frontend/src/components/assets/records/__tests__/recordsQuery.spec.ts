import { describe, expect, it } from 'vitest'
import { assetRecordListParams } from '@nosdesk/core/services/assetRecordService'
import {
  clearFilters,
  listFilter,
  periodRange,
  readQuery,
  writeQuery,
} from '@/components/assets/records/recordsQuery'

// The Records list keeps its search, filters, period, grouping and sort in
// the address bar, so a filtered list is a link, and asks the server for
// the same set.

describe('the Records query in the address bar', () => {
  it('reads back what it writes', () => {
    const raw = {
      q: 'RMA-48213907',
      type: ['repair', 'inspection'],
      status: 'open',
      vendor: ['Acme | Sons, Pty', 'Dell'],
      asset: ['12', '7'],
      period: 'this-year',
      group: 'vendor',
      sort: 'cost',
      dir: 'asc',
      record: '9101',
    }
    const query = readQuery(raw)
    expect(query.type).toEqual(new Set(['repair', 'inspection']))
    expect(query.vendor).toEqual(new Set(['Acme | Sons, Pty', 'Dell']))
    expect(query.record).toBe(9101)
    expect(writeQuery(query)).toEqual(raw)
  })

  it('leaves defaults and nonsense out', () => {
    const query = readQuery({ status: ['pending', 'open'], asset: ['CAM-1', '3'], group: 'colour', sort: 'price', dir: 'up' })
    expect([...query.status]).toEqual(['open'])
    expect([...query.asset]).toEqual(['3'])
    expect(query.group).toBe('none')
    expect(query.sort).toBe('date')
    expect(query.dir).toBe('desc')
    expect(writeQuery(query)).toEqual({ status: 'open', asset: '3' })
    expect(writeQuery(readQuery({}))).toEqual({})
  })

  it('keeps a custom range by its ends and a named period by its name', () => {
    const custom = readQuery({ from: '2024-01-01', to: 'soon' })
    expect(custom.period).toBe('custom')
    expect(writeQuery(custom)).toEqual({ from: '2024-01-01' })
    // A day that doesn't exist, or a range that ends before it starts, is
    // dropped.
    expect(writeQuery(readQuery({ from: '2024-02-31', to: '2024-03-10' }))).toEqual({ to: '2024-03-10' })
    expect(writeQuery(readQuery({ from: '2024-03-10', to: '2024-03-01' }))).toEqual({})
    // A named period wins over stray ends.
    const named = readQuery({ period: 'last-year', from: '2024-01-01' })
    expect(writeQuery(named)).toEqual({ period: 'last-year' })
  })

  it('clears the filters and keeps grouping and sort', () => {
    const query = readQuery({ q: 'x', type: 'repair', period: 'this-month', group: 'month', sort: 'vendor' })
    clearFilters(query)
    expect(writeQuery(query)).toEqual({ group: 'month', sort: 'vendor' })
  })
})

describe('the days a period covers', () => {
  const today = '2026-11-25'
  const range = (period: string) => periodRange(readQuery({ period }), today)

  it('leaves a named period that runs to today open at the end', () => {
    expect(range('last-30')).toEqual({ from: '2026-10-27', to: '' })
    expect(range('this-month')).toEqual({ from: '2026-11-01', to: '' })
    expect(range('this-year')).toEqual({ from: '2026-01-01', to: '' })
    expect(range('last-year')).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(range('')).toEqual({ from: '', to: '' })
  })

  it('crosses a year for the last 30 days in January', () => {
    expect(periodRange(readQuery({ period: 'last-30' }), '2026-01-10')).toEqual({
      from: '2025-12-12',
      to: '',
    })
  })
})

describe('what the list asks the server for', () => {
  it('sends each chosen value under its own key, in a stable order', () => {
    const filter = listFilter(
      readQuery({ q: ' RMA ', type: ['repair', 'inspection'], vendor: ['Dell', 'Acme, Pty'], asset: ['12', '7'], period: 'last-year', group: 'type' }),
      '2026-11-25',
    )
    expect(filter).toMatchObject({
      q: 'RMA',
      type: ['inspection', 'repair'],
      vendor: ['Acme, Pty', 'Dell'],
      asset: [7, 12],
      from: '2025-01-01',
      to: '2025-12-31',
      group: 'type',
    })
    const params = assetRecordListParams({ ...filter, offset: 100, limit: 100 })
    expect(params.getAll('vendor')).toEqual(['Acme, Pty', 'Dell'])
    expect(params.getAll('asset')).toEqual(['7', '12'])
    expect(params.get('offset')).toBe('100')
  })

  it('asks for every record without a filter, ungrouped', () => {
    const params = assetRecordListParams(listFilter(readQuery({}), '2026-11-25'))
    expect([...params.keys()]).toEqual(['sort', 'dir'])
  })
})
