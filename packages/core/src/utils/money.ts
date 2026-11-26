/**
 * Money display. Amounts arrive from the server as a plain decimal string
 * in their own currency's minor units (`"1250.50"` AUD, `"1250"` JPY,
 * `"1.250"` KWD), with an ISO 4217 code. The server owns each currency's
 * exponent (`backend/src/utils/money.rs`); this formats what it sent and
 * never derives digits from `Intl`, whose data can disagree with ISO.
 */
import { getDateConfig } from './dateUtils'

const DECIMAL = /^-?\d+(\.\d+)?$/

/**
 * `amount` in `currency` for the active locale: `"1250.50"` AUD is
 * `A$1,250.50` in en-US and `1 250,50 $AU` in fr-FR. Shows exactly the
 * decimals the amount carries. Falls back to `"1250.50 AUD"` for an
 * amount or code `Intl` can't take.
 */
export function formatMoney(amount: string, currency: string, locale?: string): string {
  return formatAmount(amount, currency, 'symbol', locale)
}

/**
 * `amount` in `currency` with the currency's code rather than its symbol:
 * `AUD 1,250.50` in en-US and en-AU, `1 250,50 AUD` in fr-FR. For totals,
 * which always say which currency they are in, the workspace's own too,
 * where a bare `$` could be any dollar.
 */
export function formatMoneyWithCode(amount: string, currency: string, locale?: string): string {
  return formatAmount(amount, currency, 'code', locale)
}

function formatAmount(
  amount: string,
  currency: string,
  currencyDisplay: 'symbol' | 'code',
  locale?: string,
): string {
  const fraction = amount.split('.')[1]?.length ?? 0
  if (!DECIMAL.test(amount)) return `${amount} ${currency}`
  try {
    const format = new Intl.NumberFormat(locale ?? getDateConfig().defaultLocale, {
      style: 'currency',
      currency,
      currencyDisplay,
      minimumFractionDigits: fraction,
      maximumFractionDigits: fraction,
    })
    // A decimal string formats exactly, beyond a double's precision; the
    // DOM lib types `format` as taking numbers only.
    return format.format(amount as unknown as number)
  } catch {
    return `${amount} ${currency}`
  }
}

/** The currency's name in the active locale (`AUD`: "Australian Dollar"), or the code. */
export function currencyName(currency: string, locale?: string): string {
  try {
    return (
      new Intl.DisplayNames(locale ?? getDateConfig().defaultLocale, { type: 'currency' }).of(
        currency,
      ) ?? currency
    )
  } catch {
    return currency
  }
}

/** Why typed text isn't an amount. */
export type AmountInputError =
  /** Not a number in the reader's locale. */
  | 'invalid'
  /** Could be read two ways (`1.250` in fr-FR: a thousand and more, or one
   *  and a quarter). */
  | 'ambiguous'
  /** Below zero. */
  | 'negative'
  /** More decimals than the currency has. */
  | 'too_many_decimals'

export type AmountInput = { ok: true; amount: string } | { ok: false; error: AmountInputError }

/** Spaces people and `Intl` put between groups of three digits. */
const GROUP_SPACES = [' ', ' ', ' ', ' ']

/** The locale's decimal mark and group separator. */
function separators(locale: string): { decimal: string; group: string } {
  const parts = new Intl.NumberFormat(locale).formatToParts(12345.6)
  return {
    decimal: parts.find((p) => p.type === 'decimal')?.value ?? '.',
    group: parts.find((p) => p.type === 'group')?.value ?? ',',
  }
}

/**
 * What someone typed as an amount, as the canonical decimal the server
 * reads (`"1250.50"`), using the decimal mark and group separator of their
 * locale: `1,250` is a thousand and more in en-US, `12,50` is twelve and a
 * half in fr-FR and nl-NL, `1 250,50` reads in fr-FR. A currency symbol or
 * code around the number is passed over.
 *
 * Text that could be read two ways is refused rather than guessed: the
 * other punctuation mark alone before three digits (`1.250` in fr-FR), or,
 * where the decimal mark is a comma, one comma before three digits
 * (`1,250` in nl-NL), as the server's own reader refuses it, unless
 * `exponent` is 3, where three decimals are the norm. Pass `exponent` (the
 * currency's decimals) to refuse more decimals here; without it the server
 * checks them.
 */
export function readAmountInput(
  text: string,
  options: { locale?: string; exponent?: number } = {},
): AmountInput {
  const refuse = (error: AmountInputError): AmountInput => ({ ok: false, error })
  const { decimal, group } = separators(options.locale ?? getDateConfig().defaultLocale)
  const other = decimal === '.' ? ',' : '.'
  const groups = new Set([group, ...GROUP_SPACES])

  // A sign, a symbol or code before the number (`$`, `A$`, `USD`), a sign
  // after it, and a symbol or code after the number (`€`, `AUD`).
  const prefix = /^(?:[A-Z]{0,3}\p{Sc}|[A-Z]{3})\s*/u
  let body = text.trim()
  let negative = false
  const sign = () => {
    if (body.startsWith('-') && !negative) {
      negative = true
      body = body.slice(1).trim()
    } else if (body.startsWith('+')) {
      body = body.slice(1).trim()
    }
  }
  sign()
  body = body.replace(prefix, '')
  sign()
  body = body.replace(/\s*(?:\p{Sc}|[A-Z]{3})$/u, '').trim()
  if (!body) return refuse('invalid')

  // The other mark where the locale doesn't group with it: one of them
  // before three digits could be either reading.
  if (!groups.has(other) && body.includes(other)) {
    return new RegExp(`^\\d{1,3}\\${other}\\d{3}$`).test(body)
      ? refuse('ambiguous')
      : refuse('invalid')
  }

  const pieces = body.split(decimal)
  if (pieces.length > 2) return refuse('invalid')
  const [whole, fraction = ''] = pieces
  if (!/^\d*$/.test(fraction)) return refuse('invalid')
  let digits: string
  const separator = [...whole].find((c) => !/\d/.test(c))
  if (separator !== undefined) {
    // One separator throughout, a lead group of one to three digits, then
    // groups of three.
    if (!groups.has(separator)) return refuse('invalid')
    const parts = whole.split(separator)
    if (!/^\d{1,3}$/.test(parts[0]) || parts.slice(1).some((g) => !/^\d{3}$/.test(g))) {
      return refuse('invalid')
    }
    digits = parts.join('')
  } else {
    if (whole === '' && fraction === '') return refuse('invalid')
    // In a currency with three decimals (KWD, BHD) three after the comma
    // is how amounts are written, so the comma is the decimal mark.
    if (
      decimal === ',' &&
      fraction.length === 3 &&
      options.exponent !== 3 &&
      /^[1-9]\d{0,2}$/.test(whole)
    ) {
      return refuse('ambiguous')
    }
    digits = whole
  }
  if (negative && /[1-9]/.test(digits + fraction)) return refuse('negative')

  let kept = fraction
  if (options.exponent !== undefined) {
    if (/[1-9]/.test(fraction.slice(options.exponent))) return refuse('too_many_decimals')
    kept = fraction.slice(0, options.exponent)
  }
  const unit = digits.replace(/^0+(?=\d)/, '') || '0'
  return { ok: true, amount: kept ? `${unit}.${kept}` : unit }
}
