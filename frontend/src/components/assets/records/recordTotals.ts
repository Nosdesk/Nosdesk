// How the Records list writes a total: always with its currency's code,
// the workspace currency's too, so totals in several currencies read
// side by side and a bare `$` never leaves the reader guessing.
import type { AssetRecordCurrencyTotal } from '@nosdesk/core/types/asset'
import { formatMoneyWithCode } from '@nosdesk/core/utils/money'

/** `total` with its code: `AUD 1,250.50` in en-AU, `1 250,50 AUD` in
 *  fr-FR. A code the server couldn't format shows its minor units. */
export function totalText(total: AssetRecordCurrencyTotal, locale?: string): string {
  if (total.amount === null) return `${total.amount_minor} ${total.currency}`
  return formatMoneyWithCode(total.amount, total.currency, locale)
}
