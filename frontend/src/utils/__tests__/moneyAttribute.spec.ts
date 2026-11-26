import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { formatMoney } from '@nosdesk/core/utils/money'
import {
  isMoneyProp,
  localAmount,
  moneyCurrency,
  moneyText,
  moneyValueOf,
  readMoneyInput,
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

  it('shows a bare amount without a currency and nothing for anything else', () => {
    expect(moneyText({ amount: '12.5' })).toBe('12.5')
    expect(moneyText({ amount: 7, currency: 'USD' })).toBe('$7')
    expect(moneyText('12.50')).toBe('')
    expect(moneyText(null)).toBe('')
    expect(moneyText({ currency: 'AUD' })).toBe('')
    expect(moneyValueOf([1, 2])).toBeNull()
  })

  it("edits an amount with the reader's decimal mark", () => {
    expect(localAmount('1250.50')).toBe('1250.50')
    expect(localAmount('1250.50', 'fr-FR')).toBe('1250,50')
    expect(localAmount('1250.50', 'nl-NL')).toBe('1250,50')
  })

  it("takes the pinned currency, else the value's own, else the workspace's", () => {
    const value = { amount: '10.00', currency: 'NZD' }
    expect(moneyCurrency({ currency: 'USD' }, value, 'AUD')).toBe('USD')
    expect(moneyCurrency({}, value, 'AUD')).toBe('NZD')
    expect(moneyCurrency({}, undefined, 'AUD')).toBe('AUD')
    expect(moneyCurrency({}, undefined, '')).toBeNull()
    expect(moneyCurrency({}, undefined, null)).toBeNull()
  })

  it('reads typed text as the value to write', () => {
    expect(readMoneyInput('1,250.5', 'AUD')).toEqual({ ok: true, value: { amount: '1250.5', currency: 'AUD' } })
    expect(readMoneyInput('1 250,50', 'EUR', 'fr-FR')).toEqual({
      ok: true,
      value: { amount: '1250.50', currency: 'EUR' },
    })
    // No currency known: left out for the server to fill, never sent empty.
    expect(readMoneyInput('40', null)).toEqual({ ok: true, value: { amount: '40' } })
    expect(readMoneyInput('   ', 'AUD')).toEqual({ ok: true, value: null })
    expect(readMoneyInput('forty', 'AUD')).toEqual({ ok: false, error: 'invalid' })
    expect(readMoneyInput('-4', 'AUD')).toEqual({ ok: false, error: 'negative' })
  })
})
