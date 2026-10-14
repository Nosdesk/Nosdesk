/**
 * Cell content for the inventory list's Borrower and Last status change
 * columns. Both read fields the paginated list computes server side
 * (`current_loan`, `latest_lifecycle`), so the table makes no request per row.
 */
import type { AssetCurrentLoan, AssetLatestLifecycle } from '@nosdesk/core/types/asset'
import { formatCompactDate, formatDate, formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { lifecycleTransitionLabel } from '@/utils/assetStatusMeta'

type Translate = (key: string, args?: Record<string, string>) => string

export type BorrowerCell =
  | { kind: 'none' }
  /** On loan to an account that no longer exists. */
  | { kind: 'unknown'; title: string }
  | { kind: 'user'; uuid: string; name: string; title: string }

/** What the Borrower cell shows: nothing without an open loan, the borrower
 *  when known, else a muted "Loaned". The title gives the loan dates. */
export function borrowerCell(
  t: Translate,
  loan: AssetCurrentLoan | null | undefined,
): BorrowerCell {
  if (!loan) return { kind: 'none' }
  const issued = formatDate(loan.issued_at)
  // A due date is a calendar day; format it in UTC so it can't shift.
  const title = loan.due_at
    ? t('assets-list-borrower-title-due', { issued, due: formatDate(loan.due_at, 'date', 'UTC') })
    : t('assets-list-borrower-title', { issued })
  if (loan.borrower_user_uuid && loan.borrower_name) {
    return { kind: 'user', uuid: loan.borrower_user_uuid, name: loan.borrower_name, title }
  }
  return { kind: 'unknown', title: `${t('assets-list-borrower-unknown-title')}\n${title}` }
}

export interface LastStatusChangeCell {
  label: string
  date: string
  title: string
}

/** What the Last status change cell shows: the transition and a short date,
 *  with the full date and, for staff, who made it and why in the title. */
export function lastStatusChangeCell(
  t: Translate,
  event: AssetLatestLifecycle | null | undefined,
): LastStatusChangeCell | null {
  if (!event) return null
  const label = lifecycleTransitionLabel(t, event.from_status, event.to_status)
  const when = formatDateTime(event.occurred_at)
  const lines = [
    label,
    event.actor_name ? t('assets-list-last-status-change-by', { when, name: event.actor_name }) : when,
  ]
  if (event.reason) lines.push(t('assets-list-last-status-change-reason', { reason: event.reason }))
  return { label, date: formatCompactDate(event.occurred_at), title: lines.join('\n') }
}
