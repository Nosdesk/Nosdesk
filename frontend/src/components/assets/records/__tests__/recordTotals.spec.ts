import { describe, expect, it } from 'vitest'
import { totalText } from '@/components/assets/records/recordTotals'

// Totals always name their currency by its code, the workspace currency's
// too: in en-AU a bare `$` would be the only total without one.

const total = (amount: string | null, currency: string, amount_minor = 0) => ({
  amount,
  currency,
  amount_minor,
})

describe('a total', () => {
  it('carries the workspace currency code where the locale would write a symbol', () => {
    const aud = totalText(total('1250.50', 'AUD'), 'en-AU')
    expect(aud).toContain('AUD')
    expect(aud).not.toContain('$')
    expect(aud).toMatch(/1,250\.50/)
    const usd = totalText(total('99.00', 'USD'), 'en-US')
    expect(usd).toContain('USD')
    expect(usd).not.toContain('$')
  })

  it('carries a foreign currency code too, in the locale of the reader', () => {
    expect(totalText(total('320.00', 'USD'), 'en-AU')).toContain('USD')
    const eur = totalText(total('1234.56', 'EUR'), 'fr-FR')
    expect(eur).toContain('EUR')
    expect(eur).not.toContain('€')
    expect(eur).toMatch(/1\s?234,56/)
  })

  it('keeps the decimals the server sent', () => {
    expect(totalText(total('1250', 'JPY'), 'en-US')).toMatch(/1,250(?!\.)/)
    expect(totalText(total('1.250', 'KWD'), 'en-US')).toContain('1.250')
  })

  it('falls back to minor units for a code the server could not format', () => {
    expect(totalText(total(null, 'XYZ', 4200))).toBe('4200 XYZ')
  })
})
