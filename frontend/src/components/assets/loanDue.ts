import type { StatusPillTone } from '@/components/common/statusPillTone';
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils';
import type { AssetLoan } from '@nosdesk/core/types/asset';

export interface LoanDue {
  label: string;
  tone: StatusPillTone;
}

type Translate = (key: string, args?: Record<string, string | number>) => string;

/**
 * The due-date pill for a loan that's still out: overdue, due today, due
 * within two days, or the date. `null` for a returned or open-ended loan.
 */
export function loanDue(loan: AssetLoan, t: Translate): LoanDue | null {
  if (loan.returned_at || !loan.due_back) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const due = new Date(`${loan.due_back}T00:00:00`);
  const days = Math.round((due.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { label: t('asset-loan-due-overdue'), tone: 'critical' };
  if (days === 0) return { label: t('asset-loan-due-today'), tone: 'caution' };
  if (days <= 2) return { label: t('asset-loan-due-soon', { days }), tone: 'caution' };
  return { label: t('asset-loan-due-on', { date: formatCompactDate(loan.due_back) }), tone: 'neutral' };
}
