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
  const fraction = amount.split('.')[1]?.length ?? 0
  if (!DECIMAL.test(amount)) return `${amount} ${currency}`
  try {
    const format = new Intl.NumberFormat(locale ?? getDateConfig().defaultLocale, {
      style: 'currency',
      currency,
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
