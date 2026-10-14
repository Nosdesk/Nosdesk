import { beforeAll, describe, expect, it } from 'vitest'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { borrowerCell, lastStatusChangeCell } from '@/utils/assetLoanStatusColumns'

// Echo the key and its arguments so assertions see what was asked for.
const t = (key: string, args?: Record<string, string>) =>
  args ? `${key}(${Object.entries(args).map(([k, v]) => `${k}=${v}`).join(',')})` : key

beforeAll(() => setDateConfig({ defaultLocale: 'en-US', timezone: 'UTC' }))

describe('borrowerCell', () => {
  it('is empty without an open loan', () => {
    expect(borrowerCell(t, null)).toEqual({ kind: 'none' })
    expect(borrowerCell(t, undefined)).toEqual({ kind: 'none' })
  })

  it('names the borrower, with the loan dates in the title', () => {
    expect(
      borrowerCell(t, {
        borrower_user_uuid: 'u-1',
        borrower_name: 'Mia',
        issued_at: '2026-10-01T09:00:00Z',
        due_at: '2026-12-01',
      }),
    ).toEqual({
      kind: 'user',
      uuid: 'u-1',
      name: 'Mia',
      title: 'assets-list-borrower-title-due(issued=Oct 1, 2026,due=Dec 1, 2026)',
    })
  })

  it('reads as loaned when the borrower account is gone', () => {
    const cell = borrowerCell(t, {
      borrower_user_uuid: null,
      borrower_name: null,
      issued_at: '2026-10-01T09:00:00Z',
      due_at: null,
    })
    expect(cell).toEqual({
      kind: 'unknown',
      title: 'assets-list-borrower-unknown-title\nassets-list-borrower-title(issued=Oct 1, 2026)',
    })
  })
})

describe('lastStatusChangeCell', () => {
  it('is empty without an event', () => {
    expect(lastStatusChangeCell(t, null)).toBeNull()
  })

  it('labels a transition and puts who and why in the title', () => {
    const cell = lastStatusChangeCell(t, {
      from_status: 'in_repair',
      to_status: 'in_service',
      reason: 'Screen replaced',
      occurred_at: '2025-06-01T09:00:00Z',
      actor_uuid: 'u-2',
      actor_name: 'Ana',
    })
    const label =
      'asset-lifecycle-timeline-transition(from=asset-status-in-repair,to=asset-status-in-service)'
    expect(cell?.label).toBe(label)
    expect(cell?.date).toBe('Jun 1, 2025')
    expect(cell?.title.split('\n')).toEqual([
      label,
      'assets-list-last-status-change-by(when=Jun 1, 2025, 09:00 AM,name=Ana)',
      'assets-list-last-status-change-reason(reason=Screen replaced)',
    ])
  })

  it('labels a first status and leaves out what a requester does not get', () => {
    const cell = lastStatusChangeCell(t, {
      from_status: null,
      to_status: 'in_stock',
      occurred_at: '2025-06-01T09:00:00Z',
    })
    expect(cell?.label).toBe('asset-lifecycle-timeline-initial(to=asset-status-in-stock)')
    expect(cell?.title.split('\n')).toEqual([cell?.label, 'Jun 1, 2025, 09:00 AM'])
  })
})
