/**
 * Money in attributes and record fields: a property `{ type: "object",
 * format: "money" }`, valued `{ amount: "1250.50", currency: "AUD" }`
 * (`backend/src/services/custom_fields/money.rs`). The server fills a
 * missing currency from the property's pinned code, else the workspace's,
 * and writes the amount canonical to its currency, so a write may leave the
 * currency out but never sends it empty.
 */
import { getDateConfig } from '@nosdesk/core/utils/dateUtils'
import { formatMoney, readAmountInput, type AmountInputError } from '@nosdesk/core/utils/money'

/** A money value as the server stores it. */
export interface MoneyValue {
  amount: string
  currency: string
}

/** A money value to write: the currency left out for the server to fill. */
export interface MoneyWrite {
  amount: string
  currency?: string
}

/** Whether a schema property holds money. */
export function isMoneyProp(prop: { type?: unknown; format?: unknown } | null | undefined): boolean {
  return prop?.type === 'object' && prop.format === 'money'
}

/** A stored money value read loosely: the amount as text (a whole JSON
 *  number too), the currency when it has one. Null for anything else. */
export function moneyValueOf(value: unknown): { amount: string; currency: string | null } | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const { amount, currency } = value as Record<string, unknown>
  const text =
    typeof amount === 'string' && amount.trim() !== ''
      ? amount.trim()
      : typeof amount === 'number' && Number.isFinite(amount)
        ? String(amount)
        : null
  if (text === null) return null
  return { amount: text, currency: typeof currency === 'string' && currency.trim() ? currency.trim() : null }
}

/** A money value as text: `A$1,250.50`, the bare amount when it has no
 *  currency, and empty for anything that isn't money. */
export function moneyText(value: unknown, locale?: string): string {
  const money = moneyValueOf(value)
  if (!money) return ''
  return money.currency ? formatMoney(money.amount, money.currency, locale) : money.amount
}

/** An amount with the reader's decimal mark, to edit: `"1250.50"` is
 *  `"1250,50"` in fr-FR, which reads back as the same amount. */
export function localAmount(amount: string, locale?: string): string {
  const decimal =
    new Intl.NumberFormat(locale ?? getDateConfig().defaultLocale)
      .formatToParts(1.5)
      .find((p) => p.type === 'decimal')?.value ?? '.'
  return amount.replace('.', decimal)
}

/** The currency a money field is entered in: the property's pinned code,
 *  else the value's own (an edit never changes it), else `fallback` (the
 *  workspace's). Null when none is known, for the server to fill. */
export function moneyCurrency(
  prop: { currency?: unknown } | null | undefined,
  value: unknown,
  fallback?: string | null,
): string | null {
  if (typeof prop?.currency === 'string' && prop.currency) return prop.currency
  return moneyValueOf(value)?.currency ?? (fallback || null)
}

export type MoneyInput =
  | { ok: true; value: MoneyWrite | null }
  | { ok: false; error: AmountInputError }

/** What someone typed, as the value to write: null for an empty field,
 *  else the amount read in their locale with `currency` when one is known. */
export function readMoneyInput(
  text: string,
  currency: string | null | undefined,
  locale?: string,
): MoneyInput {
  if (!text.trim()) return { ok: true, value: null }
  const read = readAmountInput(text, { locale })
  if (!read.ok) return read
  return { ok: true, value: currency ? { amount: read.amount, currency } : { amount: read.amount } }
}

/** The translation key for why typed text isn't an amount. */
export const AMOUNT_ERROR_KEYS: Record<AmountInputError, string> = {
  invalid: 'asset-lifecycle-record-cost-invalid',
  ambiguous: 'asset-lifecycle-record-cost-ambiguous',
  negative: 'asset-lifecycle-record-cost-negative',
  too_many_decimals: 'asset-lifecycle-record-cost-invalid',
}
