import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import { mountWithProviders } from '@/test/mountWithProviders'
import DynamicAttributeForm from '../DynamicAttributeForm.vue'

// A money attribute is an amount in a currency: the form shows the amount
// with the currency's code, never `[object Object]`, and reports
// `{ amount, currency }` once the amount reads, on blur or Enter, so the
// asset page's save on every change never sends a string.

const SCHEMA = {
  type: 'object',
  properties: {
    price: { type: 'object', format: 'money', title: 'Purchase price' },
    usd: { type: 'object', format: 'money', currency: 'USD' },
  },
}

function render(modelValue: Record<string, unknown>, extra: Record<string, unknown> = {}) {
  const wrapper = mountWithProviders(DynamicAttributeForm, { schema: SCHEMA, modelValue, ...extra })
  const form = wrapper.findComponent(DynamicAttributeForm)
  const inputs = wrapper.findAll('input')
  const updates = () => (form.emitted('update:modelValue') ?? []).map((args) => args[0])
  return { wrapper, inputs, updates }
}

const saved = getDateConfig()
beforeEach(() => setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' }))
afterEach(() => setDateConfig(saved))

describe('DynamicAttributeForm money fields', () => {
  it('shows the amount with its currency, not the object', () => {
    const { wrapper, inputs } = render({ price: { amount: '1250.50', currency: 'AUD' } }, { currency: 'NZD' })
    expect((inputs[0].element as HTMLInputElement).value).toBe('1250.50')
    const codes = wrapper.findAll('[data-testid="money-currency"]').map((c) => c.text())
    // The value's own currency, and the pinned one for the empty field.
    expect(codes).toEqual(['AUD', 'USD'])
    expect(wrapper.text()).not.toContain('[object Object]')
  })

  it("reports the typed amount in the value's currency on blur", async () => {
    const { inputs, updates } = render({ price: { amount: '1250.50', currency: 'AUD' }, note: 'kept' })
    await inputs[0].setValue('1,300.5')
    expect(updates()).toEqual([])
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ price: { amount: '1300.5', currency: 'AUD' }, note: 'kept' }])
  })

  it("reports a new amount in the pinned currency, else the workspace's, on Enter", async () => {
    const { inputs, updates } = render({}, { currency: 'NZD' })
    await inputs[0].setValue('40')
    await inputs[0].trigger('keydown', { key: 'Enter' })
    await inputs[1].setValue('12.5')
    await inputs[1].trigger('keydown', { key: 'Enter' })
    expect(updates()).toEqual([{ price: { amount: '40', currency: 'NZD' } }, { usd: { amount: '12.5', currency: 'USD' } }])
  })

  it('leaves the currency for the server when none is known', async () => {
    const { inputs, updates } = render({})
    await inputs[0].setValue('40')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ price: { amount: '40' } }])
  })

  it('reports nothing for an amount that does not read, and says why', async () => {
    const { wrapper, inputs, updates } = render({ price: { amount: '10.00', currency: 'AUD' } })
    await inputs[0].setValue('ten')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    expect(wrapper.text()).toContain('asset-lifecycle-record-cost-invalid')
  })

  it('drops the key when the amount is cleared, and reports nothing unchanged', async () => {
    const { inputs, updates } = render({ price: { amount: '10.00', currency: 'AUD' }, note: 'kept' })
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    await inputs[0].setValue('')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ note: 'kept' }])
  })
})
