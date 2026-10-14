/**
 * Cell content for the inventory list's Borrower and Last status change
 * columns. Both read fields the paginated list computes server side
 * (`current_loan`, `latest_lifecycle`), so the table makes no request per row.
 */
import type { AssetCurrentLoan, AssetLatestLifecycle } from '@nosdesk/core/types/asset'
import { formatCompactDate, formatDate, formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { assetStatusLabel, lifecycleTransitionLabel } from '@/utils/assetStatusMeta'
import { loanDaysLeft } from '@/components/assets/loanDue'

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
  const title = loanDates(t, loan)
  if (loan.borrower_user_uuid && loan.borrower_name) {
    // The name leads the title: a narrow column truncates it.
    const named = `${loan.borrower_name}\n${title}`
    return { kind: 'user', uuid: loan.borrower_user_uuid, name: loan.borrower_name, title: named }
  }
  return { kind: 'unknown', title: `${t('assets-list-borrower-unknown-title')}\n${title}` }
}

/** "Loaned 1 Oct 2026, due back 1 Dec 2026", or without the due date. */
function loanDates(t: Translate, loan: AssetCurrentLoan): string {
  const loaned = formatDate(loan.loaned_at)
  // A due date is a calendar day; format it in UTC so it can't shift.
  return loan.due_back
    ? t('assets-list-borrower-title-due', { loaned, due: formatDate(loan.due_back, 'date', 'UTC') })
    : t('assets-list-borrower-title', { loaned })
}

export type UsedByCell =
  | { kind: 'unassigned' }
  | { kind: 'user'; uuid: string; name: string }
  /** Out on loan: the borrower has it, whoever it's assigned to. A null
   *  uuid is a borrower whose account is gone. */
  | { kind: 'loan'; uuid: string | null; name: string | null; overdue: boolean; title: string }

/** What the Used by cell shows: while an asset is on loan, the borrower,
 *  since they're the one using it; otherwise the assigned user. The title
 *  gives the loan dates and, during a loan, who the asset is assigned to. */
export function usedByCell(
  t: Translate,
  primaryUser: { uuid: string; name: string } | null | undefined,
  loan: AssetCurrentLoan | null | undefined,
): UsedByCell {
  if (!loan) {
    return primaryUser ? { kind: 'user', uuid: primaryUser.uuid, name: primaryUser.name } : { kind: 'unassigned' }
  }
  const named = !!(loan.borrower_user_uuid && loan.borrower_name)
  const lines = [
    named
      ? t('assets-list-user-on-loan-title', { name: loan.borrower_name! })
      : t('assets-list-borrower-unknown-title'),
    loanDates(t, loan),
  ]
  if (primaryUser) lines.push(t('assets-list-user-assigned-title', { name: primaryUser.name }))
  return {
    kind: 'loan',
    uuid: named ? loan.borrower_user_uuid : null,
    name: named ? loan.borrower_name : null,
    overdue: (loanDaysLeft(loan) ?? 0) < 0,
    title: lines.join('\n'),
  }
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
