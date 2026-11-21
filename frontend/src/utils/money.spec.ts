import { describe, expect, it } from 'vitest'
import { currencyName, formatMoney } from '@nosdesk/core/utils/money'

// Expectations come from the runtime's own Intl, so the specs hold whatever
// ICU data the Node version ships; they pin what our code decides: the
// fraction digits come from the server's string, not Intl's currency data.
function intl(value: number, currency: string, locale: string, digits: number): string {
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency,
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value)
}

/** The fraction digits in a formatted amount, by its decimal part. */
function fractionOf(formatted: string, currency: string, locale: string): string {
  const parts = new Intl.NumberFormat(locale, { style: 'currency', currency }).formatToParts(0.5)
  const decimal = parts.find((p) => p.type === 'decimal')?.value ?? '.'
  const after = formatted.split(decimal)[1] ?? ''
  return after.replace(/\D/g, '')
}

describe('formatMoney', () => {
  it('shows exactly the decimals the server sent', () => {
    expect(formatMoney('1250.50', 'AUD', 'en-AU')).toBe(intl(1250.5, 'AUD', 'en-AU', 2))
    expect(formatMoney('1250', 'JPY', 'en-US')).toBe(intl(1250, 'JPY', 'en-US', 0))
    expect(formatMoney('1.250', 'KWD', 'en-US')).toBe(intl(1.25, 'KWD', 'en-US', 3))
    expect(fractionOf(formatMoney('1.250', 'KWD', 'en-US'), 'KWD', 'en-US')).toBe('250')
  })

  it('keeps ISO digits where Intl has other ones', () => {
    // IDR has two minor units in ISO 4217; Intl's currency data shows none.
    const formatted = formatMoney('1500.25', 'IDR', 'en-US')
    expect(formatted).toBe(intl(1500.25, 'IDR', 'en-US', 2))
    expect(fractionOf(formatted, 'IDR', 'en-US')).toBe('25')
  })

  it('keeps the sign and follows the locale', () => {
    expect(formatMoney('-12.50', 'USD', 'en-US')).toBe(intl(-12.5, 'USD', 'en-US', 2))
    expect(formatMoney('1234.56', 'EUR', 'fr-FR')).toBe(intl(1234.56, 'EUR', 'fr-FR', 2))
    expect(formatMoney('1234.56', 'EUR', 'nl-NL')).toBe(intl(1234.56, 'EUR', 'nl-NL', 2))
  })

  it('keeps every digit of an amount beyond a double', () => {
    const formatted = formatMoney('92233720368547758.07', 'USD', 'en-US')
    expect(formatted.replace(/\D/g, '')).toBe('9223372036854775807')
  })

  it('falls back to the amount and code', () => {
    expect(formatMoney('12.5', 'NOT A CODE', 'en-US')).toBe('12.5 NOT A CODE')
    expect(formatMoney('1e5', 'USD', 'en-US')).toBe('1e5 USD')
  })
})

describe('currencyName', () => {
  it('names the currency in the locale, or gives the code', () => {
    const named = new Intl.DisplayNames('en-US', { type: 'currency' }).of('AUD')
    expect(currencyName('AUD', 'en-US')).toBe(named)
    expect(currencyName('AUD', 'en-US')).not.toBe('AUD')
    expect(currencyName('??', 'en-US')).toBe('??')
  })
})
