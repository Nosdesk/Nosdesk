import { describe, expect, it } from 'vitest'
import { currencyName, formatMoney, readAmountInput } from '@nosdesk/core/utils/money'

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

describe('readAmountInput', () => {
  const read = (text: string, locale: string, exponent?: number) =>
    readAmountInput(text, { locale, exponent })
  const amount = (value: string) => ({ ok: true, amount: value })
  const refused = (error: string) => ({ ok: false, error })

  it('reads en-US and en-AU with a point for decimals and commas between thousands', () => {
    for (const locale of ['en-US', 'en-AU']) {
      expect(read('1,250', locale)).toEqual(amount('1250'))
      expect(read('1,250.50', locale)).toEqual(amount('1250.50'))
      expect(read('  $1,234,567.8 ', locale)).toEqual(amount('1234567.8'))
      expect(read('A$12', locale)).toEqual(amount('12'))
      expect(read('120.50 AUD', locale)).toEqual(amount('120.50'))
      expect(read('.5', locale)).toEqual(amount('0.5'))
      expect(read('007', locale)).toEqual(amount('7'))
      // A comma that can't group three digits isn't a decimal mark here.
      expect(read('12,50', locale)).toEqual(refused('invalid'))
      expect(read('1,25,000', locale)).toEqual(refused('invalid'))
    }
  })

  it('reads fr-FR with a comma for decimals and spaces between thousands', () => {
    expect(read('12,50', 'fr-FR')).toEqual(amount('12.50'))
    expect(read('1 250,50 €', 'fr-FR')).toEqual(amount('1250.50'))
    expect(read('1\u202f250', 'fr-FR')).toEqual(amount('1250'))
    // A point before three digits: a thousand and more, or one and a quarter?
    expect(read('1.250', 'fr-FR')).toEqual(refused('ambiguous'))
    expect(read('12.50', 'fr-FR')).toEqual(refused('invalid'))
    // One comma before three digits reads either way too.
    expect(read('1,250', 'fr-FR')).toEqual(refused('ambiguous'))
  })

  it('reads nl-NL with a comma for decimals and points between thousands', () => {
    expect(read('12,50', 'nl-NL')).toEqual(amount('12.50'))
    expect(read('1.250', 'nl-NL')).toEqual(amount('1250'))
    expect(read('€ 1.250,50', 'nl-NL')).toEqual(amount('1250.50'))
    expect(read('0,250', 'nl-NL')).toEqual(amount('0.250'))
    expect(read('1,250', 'nl-NL')).toEqual(refused('ambiguous'))
    expect(read('1.25', 'nl-NL')).toEqual(refused('invalid'))
  })

  it('takes no decimals in a zero-exponent currency when told its exponent', () => {
    expect(read('¥1,250', 'en-US', 0)).toEqual(amount('1250'))
    expect(read('1250.00', 'en-US', 0)).toEqual(amount('1250'))
    expect(read('12.5', 'en-US', 0)).toEqual(refused('too_many_decimals'))
    expect(read('12,5', 'nl-NL', 0)).toEqual(refused('too_many_decimals'))
    expect(read('12.345', 'en-US', 2)).toEqual(refused('too_many_decimals'))
  })

  it('reads three decimals after a comma in a three-decimal currency', () => {
    expect(read('2,250', 'nl-NL', 3)).toEqual(amount('2.250'))
    expect(read('2,250', 'fr-FR', 3)).toEqual(amount('2.250'))
    expect(read('2,250', 'nl-NL', 2)).toEqual(refused('ambiguous'))
    // The point alone before three digits still reads two ways in fr-FR.
    expect(read('2.250', 'fr-FR', 3)).toEqual(refused('ambiguous'))
  })

  it('refuses what is not an amount, and anything below zero', () => {
    for (const text of ['', '   ', 'abc', '12abc', '1.2.3', '--5']) {
      expect(read(text, 'en-US')).toEqual(refused('invalid'))
    }
    expect(read('-5', 'en-US')).toEqual(refused('negative'))
    expect(read('-$5', 'en-US')).toEqual(refused('negative'))
    expect(read('-0', 'en-US')).toEqual(amount('0'))
  })
})
