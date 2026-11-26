import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { formatMoney } from '@nosdesk/core/utils/money'
import {
  currencyExponent,
  isMoneyProp,
  isStoredMoney,
  localAmount,
  moneyCurrency,
  moneyErrorText,
  moneyText,
  moneyValueOf,
  readMoneyInput,
  sentCurrency,
  zeroAmount,
} from '@/utils/moneyAttribute'

// Money attributes are `{ amount, currency }` objects: they read as money
// in the viewer's locale, never as `[object Object]`, and what is typed
// goes back as the shape the server validates.

const saved = getDateConfig()
beforeEach(() => setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' }))
afterEach(() => setDateConfig(saved))

describe('money attributes', () => {
  it('knows a money property by its type and format', () => {
    expect(isMoneyProp({ type: 'object', format: 'money' })).toBe(true)
    expect(isMoneyProp({ type: 'object', format: 'money', currency: 'USD' })).toBe(true)
    expect(isMoneyProp({ type: 'string', format: 'money' })).toBe(false)
    expect(isMoneyProp({ type: 'number' })).toBe(false)
    expect(isMoneyProp(undefined)).toBe(false)
  })

  it('formats a stored value in its own currency', () => {
    expect(moneyText({ amount: '1250.50', currency: 'AUD' })).toBe(formatMoney('1250.50', 'AUD'))
    expect(moneyText({ amount: '1250.50', currency: 'AUD' })).toBe('A$1,250.50')
    expect(moneyText({ amount: '1250', currency: 'JPY' })).toBe('¥1,250')
    expect(moneyText({ amount: '1250.50', currency: 'EUR' }, 'fr-FR')).toBe(
      formatMoney('1250.50', 'EUR', 'fr-FR'),
    )
  })

  it('shows an amount without a currency as it is, and nothing for anything else', () => {
    expect(moneyText({ amount: '12.5' })).toBe('12.5')
    expect(moneyText({ amount: 7, currency: 'USD' })).toBe('$7')
    // Written before the field held money.
    expect(moneyText('1250')).toBe('1250')
    expect(moneyText(1250)).toBe('1250')
    expect(moneyText('about twelve')).toBe('')
    expect(moneyText(null)).toBe('')
    expect(moneyText({ currency: 'AUD' })).toBe('')
    expect(moneyValueOf([1, 2])).toBeNull()
    expect(moneyValueOf('1250')).toBeNull()
  })

  it('knows money as the server stores it, with its currency', () => {
    expect(isStoredMoney({ amount: '1.00', currency: 'AUD' })).toBe(true)
    expect(isStoredMoney({ amount: '1.00' })).toBe(false)
    expect(isStoredMoney('1250')).toBe(false)
  })

  it("edits an amount with the reader's decimal mark", () => {
    expect(localAmount('1250.50')).toBe('1250.50')
    expect(localAmount('1250.50', 'fr-FR')).toBe('1250,50')
    expect(localAmount('1250.50', 'nl-NL')).toBe('1250,50')
  })

  it("shows the pinned currency, else the value's own, else the workspace's", () => {
    const value = { amount: '10.00', currency: 'NZD' }
    expect(moneyCurrency({ currency: 'USD' }, value, 'AUD')).toBe('USD')
    expect(moneyCurrency({}, value, 'AUD')).toBe('NZD')
    expect(moneyCurrency({}, undefined, 'AUD')).toBe('AUD')
    expect(moneyCurrency({}, undefined, '')).toBeNull()
    expect(moneyCurrency({}, undefined, null)).toBeNull()
  })

  it("sends only the pinned currency or the value's own, never the workspace's", () => {
    expect(sentCurrency({ currency: 'USD' }, { amount: '1.00', currency: 'NZD' })).toBe('USD')
    expect(sentCurrency({}, { amount: '1.00', currency: 'NZD' })).toBe('NZD')
    expect(sentCurrency({}, undefined)).toBeNull()
  })

  it("knows each currency's decimals from ISO 4217, not Intl", () => {
    expect(currencyExponent('AUD')).toBe(2)
    expect(currencyExponent('JPY')).toBe(0)
    expect(currencyExponent('KWD')).toBe(3)
    // Intl says none for these; ISO says two.
    expect(currencyExponent('IDR')).toBe(2)
    expect(currencyExponent('HUF')).toBe(2)
    expect(currencyExponent(null)).toBeUndefined()
    expect(zeroAmount('AUD')).toBe('0.00')
    expect(zeroAmount('JPY')).toBe('0')
    expect(zeroAmount('KWD', 'fr-FR')).toBe('0,000')
  })

  it('reads typed text as the value to write', () => {
    expect(readMoneyInput('1,250.5', { currency: 'AUD' })).toEqual({
      ok: true,
      value: { amount: '1250.5', currency: 'AUD' },
    })
    expect(readMoneyInput('1 250,50', { currency: 'EUR', locale: 'fr-FR' })).toEqual({
      ok: true,
      value: { amount: '1250.50', currency: 'EUR' },
    })
    // No currency to send: left out for the server to fill, never sent empty.
    expect(readMoneyInput('40', { checkCurrency: 'AUD' })).toEqual({ ok: true, value: { amount: '40' } })
    expect(readMoneyInput('   ', { currency: 'AUD' })).toEqual({ ok: true, value: null })
    expect(readMoneyInput('forty', { currency: 'AUD' })).toEqual({ ok: false, error: 'invalid' })
    expect(readMoneyInput('1,250', { currency: 'EUR', locale: 'nl-NL' })).toEqual({ ok: false, error: 'ambiguous' })
  })

  it("refuses more decimals than the currency has", () => {
    expect(readMoneyInput('12.345', { currency: 'AUD' })).toEqual({ ok: false, error: 'too_many_decimals' })
    expect(readMoneyInput('12.5', { checkCurrency: 'JPY' })).toEqual({ ok: false, error: 'too_many_decimals' })
    expect(readMoneyInput('12.345', { currency: 'KWD' })).toEqual({
      ok: true,
      value: { amount: '12.345', currency: 'KWD' },
    })
  })

  it('takes below zero unless the minimum is zero or more, and keeps to the bounds', () => {
    expect(readMoneyInput('-50', { currency: 'AUD' })).toEqual({ ok: true, value: { amount: '-50', currency: 'AUD' } })
    expect(readMoneyInput('-50', { currency: 'AUD', minimum: -100 })).toEqual({
      ok: true,
      value: { amount: '-50', currency: 'AUD' },
    })
    expect(readMoneyInput('-50', { currency: 'AUD', minimum: 0 })).toEqual({ ok: false, error: 'negative' })
    expect(readMoneyInput('5', { currency: 'AUD', minimum: 10 })).toEqual({ ok: false, error: 'below_minimum' })
    expect(readMoneyInput('-150', { currency: 'AUD', minimum: -100 })).toEqual({ ok: false, error: 'below_minimum' })
    expect(readMoneyInput('1000.01', { currency: 'AUD', maximum: 1000 })).toEqual({ ok: false, error: 'above_maximum' })
    expect(readMoneyInput('1000', { currency: 'AUD', maximum: 1000 }).ok).toBe(true)
  })

  it('says why in words, with the bound as money', () => {
    const t = (key: string, args?: Record<string, string | number>) => `${key} ${JSON.stringify(args ?? {})}`
    expect(moneyErrorText(t, 'invalid', {})).toBe('asset-attribute-money-invalid {}')
    expect(moneyErrorText(t, 'below_minimum', { currency: 'AUD', minimum: 10 })).toBe(
      'asset-attribute-money-below-minimum {"min":"A$10"}',
    )
    expect(moneyErrorText(t, 'too_many_decimals', { currency: 'JPY' })).toBe(
      'asset-attribute-money-too-many-decimals {"code":"JPY","digits":0}',
    )
  })
})
