import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { mountWithProviders } from '@/test/mountWithProviders'
import AssetAttributeCell from '../AssetAttributeCell.vue'

// A money attribute in the inventory list reads as money, in its column and
// in a column another kind typed otherwise.

const saved = getDateConfig()
beforeEach(() => setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' }))
afterEach(() => setDateConfig(saved))

describe('AssetAttributeCell', () => {
  it('shows money in its own currency', () => {
    const w = mountWithProviders(AssetAttributeCell, {
      value: { amount: '1250.50', currency: 'AUD' },
      valueType: 'money',
    })
    expect(w.text()).toBe('A$1,250.50')
  })

  it('shows money held under a key another kind typed as text', () => {
    const w = mountWithProviders(AssetAttributeCell, {
      value: { amount: '40.00', currency: 'USD' },
      valueType: 'text',
    })
    expect(w.text()).toBe('$40.00')
    for (const valueType of ['date', 'datetime', 'list', 'boolean', 'user', 'asset']) {
      const cell = mountWithProviders(AssetAttributeCell, { value: { amount: '40.00', currency: 'USD' }, valueType })
      expect(cell.text()).toBe('$40.00')
    }
  })
})
