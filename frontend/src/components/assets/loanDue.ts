import type { StatusPillTone } from '@/components/common/statusPillTone';
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils';

export interface LoanDue {
  label: string;
  tone: StatusPillTone;
}

type Translate = (key: string, args?: Record<string, string | number>) => string;

/** Whole days until a loan's due date (negative once overdue), or `null`
 *  for a returned or open-ended loan. */
export function loanDaysLeft(loan: {
  due_back?: string | null;
  returned_at?: string | null;
}): number | null {
  if (loan.returned_at || !loan.due_back) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const due = new Date(`${loan.due_back}T00:00:00`);
  return Math.round((due.getTime() - today.getTime()) / 86_400_000);
}

/**
 * The due-date pill for a loan that's still out: overdue, due today, due
 * within two days, or the date. `null` for a returned or open-ended loan.
 */
export function loanDue(
  loan: { due_back?: string | null; returned_at?: string | null },
  t: Translate,
): LoanDue | null {
  const days = loanDaysLeft(loan);
  if (days === null || !loan.due_back) return null;
  if (days < 0) return { label: t('asset-loan-due-overdue'), tone: 'critical' };
  if (days === 0) return { label: t('asset-loan-due-today'), tone: 'caution' };
  if (days <= 2) return { label: t('asset-loan-due-soon', { days }), tone: 'caution' };
  return { label: t('asset-loan-due-on', { date: formatCompactDate(loan.due_back) }), tone: 'neutral' };
}
