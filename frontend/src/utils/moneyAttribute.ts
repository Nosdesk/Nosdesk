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

/** A money value as the server stores it, read loosely: the amount as text
 *  (a whole JSON number too), the currency when it has one. Null for
 *  anything else, a bare amount included. */
export function moneyValueOf(value: unknown): { amount: string; currency: string | null } | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const { amount, currency } = value as Record<string, unknown>
  const text = amountText(amount)
  if (text === null) return null
  return { amount: text, currency: typeof currency === 'string' && currency.trim() ? currency.trim() : null }
}

const DECIMAL = /^-?\d+(\.\d+)?$/

function amountText(amount: unknown): string | null {
  if (typeof amount === 'string' && DECIMAL.test(amount.trim())) return amount.trim()
  if (typeof amount === 'number' && Number.isFinite(amount)) return String(amount)
  return null
}

/** The amount a value holds: a money value's, or a bare amount (`"1250"`,
 *  `1250`) written before the field held money, with no currency. */
export function moneyAmountOf(value: unknown): { amount: string; currency: string | null } | null {
  const money = moneyValueOf(value)
  if (money) return money
  const bare = amountText(value)
  return bare === null ? null : { amount: bare, currency: null }
}

/** Whether a value is money as the server stores it, with its currency. */
export function isStoredMoney(value: unknown): boolean {
  return moneyValueOf(value)?.currency != null
}

/** A money value as text: `A$1,250.50`, a bare amount as it is, and empty
 *  for anything that isn't an amount. */
export function moneyText(value: unknown, locale?: string): string {
  const money = moneyAmountOf(value)
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

/** The currency a money field shows: the property's pinned code, else
 *  the value's own (an edit never changes it), else `fallback` (the
 *  workspace's). Null when none is known. */
export function moneyCurrency(
  prop: { currency?: unknown } | null | undefined,
  value: unknown,
  fallback?: string | null,
): string | null {
  return sentCurrency(prop, value) ?? (fallback || null)
}

/** The currency a write names: the pinned code, else the value's own. The
 *  workspace's is left to the server, which always has the current one. */
export function sentCurrency(prop: { currency?: unknown } | null | undefined, value: unknown): string | null {
  if (typeof prop?.currency === 'string' && prop.currency) return prop.currency
  return moneyValueOf(value)?.currency ?? null
}

/** ISO 4217 minor units for the currencies without two
 *  (`backend/src/utils/money.rs`); `Intl` disagrees for some. */
const EXPONENTS: Record<string, number> = {
  BHD: 3, BIF: 0, BYR: 0, CLP: 0, DJF: 0, GNF: 0, IQD: 3, ISK: 0, JOD: 3, JPY: 0, KMF: 0, KRW: 0,
  KWD: 3, LYD: 3, OMR: 3, PYG: 0, RWF: 0, TND: 3, UGX: 0, VND: 0, VUV: 0, XAF: 0, XOF: 0, XPF: 0,
}

/** The decimals a currency's amounts carry. Undefined for a code that
 *  isn't one, which the server refuses. */
export function currencyExponent(code: string | null | undefined): number | undefined {
  if (!code || !/^[A-Z]{3}$/.test(code)) return undefined
  return EXPONENTS[code] ?? 2
}

/** Zero written as the currency writes it, in the reader's locale. */
export function zeroAmount(code: string | null | undefined, locale?: string): string {
  const exponent = currencyExponent(code) ?? 2
  return localAmount(exponent > 0 ? `0.${'0'.repeat(exponent)}` : '0', locale)
}

export type MoneyInputError = AmountInputError | 'below_minimum' | 'above_maximum'

export type MoneyInput =
  | { ok: true; value: MoneyWrite | null }
  | { ok: false; error: MoneyInputError }

export interface MoneyInputOptions {
  /** The code to write; left out, the server fills the workspace's. */
  currency?: string | null
  /** The code the amount is checked against for its decimals. */
  checkCurrency?: string | null
  /** The property's bounds. Below zero is allowed unless `minimum` is zero
   *  or more. */
  minimum?: number
  maximum?: number
  locale?: string
}

/**
 * What someone typed, as the value to write: null for an empty field, else
 * the amount read in their locale, checked against the currency's decimals
 * and the property's bounds, with `currency` when one is given.
 */
export function readMoneyInput(text: string, options: MoneyInputOptions = {}): MoneyInput {
  if (!text.trim()) return { ok: true, value: null }
  const exponent = currencyExponent(options.checkCurrency ?? options.currency)
  let read = readAmountInput(text, { locale: options.locale, exponent })
  let negative = false
  if (!read.ok && read.error === 'negative') {
    // The reader refuses below zero; the property may allow it.
    read = readAmountInput(text.replace('-', ''), { locale: options.locale, exponent })
    negative = true
  }
  if (!read.ok) return read
  const amount = negative ? `-${read.amount}` : read.amount
  const number = Number(amount)
  const { minimum, maximum } = options
  if (minimum !== undefined && number < minimum) {
    return { ok: false, error: negative && minimum >= 0 ? 'negative' : 'below_minimum' }
  }
  if (maximum !== undefined && number > maximum) return { ok: false, error: 'above_maximum' }
  return { ok: true, value: options.currency ? { amount, currency: options.currency } : { amount } }
}

/** The record cost field's words for why typed text isn't an amount. */
export const AMOUNT_ERROR_KEYS: Record<AmountInputError, string> = {
  invalid: 'asset-lifecycle-record-cost-invalid',
  ambiguous: 'asset-lifecycle-record-cost-ambiguous',
  negative: 'asset-lifecycle-record-cost-negative',
  too_many_decimals: 'asset-lifecycle-record-cost-invalid',
}

type Translate = (key: string, args?: Record<string, string | number>) => string

/** Why typed text isn't an amount a money field takes, in words. */
export function moneyErrorText(
  t: Translate,
  error: MoneyInputError,
  context: { currency?: string | null; minimum?: number; maximum?: number },
): string {
  const bound = (n: number | undefined) =>
    n === undefined ? '' : context.currency ? moneyText({ amount: String(n), currency: context.currency }) : String(n)
  switch (error) {
    case 'invalid':
      return t('asset-attribute-money-invalid')
    case 'ambiguous':
      return t('asset-attribute-money-ambiguous')
    case 'negative':
      return t('asset-attribute-money-negative')
    case 'too_many_decimals':
      return t('asset-attribute-money-too-many-decimals', {
        code: context.currency ?? '',
        digits: currencyExponent(context.currency) ?? 2,
      })
    case 'below_minimum':
      return t('asset-attribute-money-below-minimum', { min: bound(context.minimum) })
    case 'above_maximum':
      return t('asset-attribute-money-above-maximum', { max: bound(context.maximum) })
  }
}
