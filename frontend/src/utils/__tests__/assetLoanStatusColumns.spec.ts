import { beforeAll, describe, expect, it } from 'vitest'
import { setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { borrowerCell, lastStatusChangeCell, usedByCell } from '@/utils/assetLoanStatusColumns'

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
        loaned_at: '2026-10-01T09:00:00Z',
        due_back: '2026-12-01',
      }),
    ).toEqual({
      kind: 'user',
      uuid: 'u-1',
      name: 'Mia',
      title: 'Mia\nassets-list-borrower-title-due(loaned=Oct 1, 2026,due=Dec 1, 2026)',
    })
  })

  it('reads as loaned when the borrower account is gone', () => {
    const cell = borrowerCell(t, {
      borrower_user_uuid: null,
      borrower_name: null,
      loaned_at: '2026-10-01T09:00:00Z',
      due_back: null,
    })
    expect(cell).toEqual({
      kind: 'unknown',
      title: 'assets-list-borrower-unknown-title\nassets-list-borrower-title(loaned=Oct 1, 2026)',
    })
  })
})

describe('lastStatusChangeCell', () => {
  it('is empty without an event', () => {
    expect(lastStatusChangeCell(t, null)).toBeNull()
  })

  it('shows where the status came from, with who and why in the title', () => {
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
    expect(cell?.date).toBe('Jun 1, 2025')
    expect(cell?.from).toBe('assets-list-status-changed-from(status=asset-status-in-repair)')
    expect(cell?.title.split('\n')).toEqual([
      label,
      'assets-list-status-changed-by(when=Jun 1, 2025, 09:00 AM,name=Ana)',
      'assets-list-status-changed-reason(reason=Screen replaced)',
    ])
  })

  it('marks a first status and leaves out what a requester does not get', () => {
    const cell = lastStatusChangeCell(t, {
      from_status: null,
      to_status: 'in_stock',
      reason: null,
      occurred_at: '2025-06-01T09:00:00Z',
      actor_uuid: null,
      actor_name: null,
    })
    expect(cell?.from).toBe('assets-list-status-changed-initial')
    expect(cell?.title.split('\n')).toEqual([
      'asset-lifecycle-timeline-initial(to=asset-status-in-stock)',
      'Jun 1, 2025, 09:00 AM',
    ])
  })
})

describe('usedByCell', () => {
  const loan = {
    borrower_user_uuid: 'u-1',
    borrower_name: 'Mia',
    loaned_at: '2026-10-01T09:00:00Z',
    due_back: '2999-12-01',
  }

  it('is the assigned user, or unassigned, without an open loan', () => {
    expect(usedByCell(t, { uuid: 'u-9', name: 'Sam' }, null)).toEqual({ kind: 'user', uuid: 'u-9', name: 'Sam' })
    expect(usedByCell(t, null, undefined)).toEqual({ kind: 'unassigned' })
  })

  it('is the borrower during a loan, naming the assigned user in the title', () => {
    expect(usedByCell(t, { uuid: 'u-9', name: 'Sam' }, loan)).toEqual({
      kind: 'loan',
      uuid: 'u-1',
      name: 'Mia',
      overdue: false,
      title: [
        'assets-list-user-on-loan-title(name=Mia)',
        'assets-list-borrower-title-due(loaned=Oct 1, 2026,due=Dec 1, 2999)',
        'assets-list-user-assigned-title(name=Sam)',
      ].join('\n'),
    })
  })

  it('is overdue once the due date has passed', () => {
    expect(usedByCell(t, null, { ...loan, due_back: '2020-01-01' })).toMatchObject({ overdue: true })
  })

  it('reads as loaned when the borrower account is gone', () => {
    expect(
      usedByCell(t, null, { ...loan, borrower_user_uuid: null, borrower_name: null, due_back: null }),
    ).toEqual({
      kind: 'loan',
      uuid: null,
      name: null,
      overdue: false,
      title: 'assets-list-borrower-unknown-title\nassets-list-borrower-title(loaned=Oct 1, 2026)',
    })
  })
})
