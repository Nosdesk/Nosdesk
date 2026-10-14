/**
 * Cell content for the inventory list's Borrower and Last status change
 * columns. Both read fields the paginated list computes server side
 * (`current_loan`, `latest_lifecycle`), so the table makes no request per row.
 */
import type { AssetCurrentLoan, AssetLatestLifecycle } from '@nosdesk/core/types/asset'
import { formatCompactDate, formatDate, formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { assetStatusLabel, lifecycleTransitionLabel } from '@/utils/assetStatusMeta'

type Translate = (key: string, args?: Record<string, string>) => string

export type BorrowerCell =
  | { kind: 'none' }
  /** On loan to an account that no longer exists. */
  | { kind: 'unknown'; title: string }
  | { kind: 'user'; uuid: string; name: string; title: string }

/** What the Borrower cell shows: nothing without an open loan, the borrower
 *  when known, else a muted "Loaned". The title gives the borrower and the
 *  loan dates. */
export function borrowerCell(
  t: Translate,
  loan: AssetCurrentLoan | null | undefined,
): BorrowerCell {
  if (!loan) return { kind: 'none' }
  const loaned = formatDate(loan.loaned_at)
  // A due date is a calendar day; format it in UTC so it can't shift.
  const title = loan.due_back
    ? t('assets-list-borrower-title-due', { loaned, due: formatDate(loan.due_back, 'date', 'UTC') })
    : t('assets-list-borrower-title', { loaned })
  if (loan.borrower_user_uuid && loan.borrower_name) {
    // The name leads the title: a narrow column truncates it.
    const named = `${loan.borrower_name}\n${title}`
    return { kind: 'user', uuid: loan.borrower_user_uuid, name: loan.borrower_name, title: named }
  }
  return { kind: 'unknown', title: `${t('assets-list-borrower-unknown-title')}\n${title}` }
}

export interface LastStatusChangeCell {
  /** Short date of the newest change. */
  date: string
  /** Where the status came from ("from In service"); the Status column
   *  already shows where it went. */
  from: string
  title: string
}

/** What the Status changed cell shows: a short date and the status it moved
 *  from. The title has the whole transition, the full date and, for staff,
 *  who made it and why. */
export function lastStatusChangeCell(
  t: Translate,
  event: AssetLatestLifecycle | null | undefined,
): LastStatusChangeCell | null {
  if (!event) return null
  const label = lifecycleTransitionLabel(t, event.from_status, event.to_status)
  const when = formatDateTime(event.occurred_at)
  const lines = [
    label,
    event.actor_name ? t('assets-list-status-changed-by', { when, name: event.actor_name }) : when,
  ]
  if (event.reason) lines.push(t('assets-list-status-changed-reason', { reason: event.reason }))
  const from = event.from_status
    ? t('assets-list-status-changed-from', { status: assetStatusLabel(t, event.from_status) })
    : t('assets-list-status-changed-initial')
  return { date: formatCompactDate(event.occurred_at), from, title: lines.join('\n') }
}
