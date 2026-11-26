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
  const invalid = () => (form.emitted('update:invalid') ?? []).map((args) => args[0])
  return { wrapper, inputs, updates, invalid }
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

  it('names the field by its label and describes it by its currency', () => {
    const { wrapper, inputs } = render({ price: { amount: '1.00', currency: 'AUD' } }, { schema: { ...SCHEMA, required: ['price'] } })
    const input = inputs[0]
    const label = wrapper.findAll('label').find((l) => l.text().startsWith('Purchase price'))
    expect(label?.attributes('for')).toBe(input.attributes('id'))
    expect(input.attributes('required')).toBeDefined()
    const code = wrapper.get(`#${input.attributes('aria-describedby')}`)
    expect(code.text()).toBe('AUD')
    // The placeholder is zero as the currency writes it.
    expect(inputs[1].attributes('placeholder')).toBe('0.00')
  })

  it("reports the typed amount in the value's currency on blur", async () => {
    const { inputs, updates } = render({ price: { amount: '1250.50', currency: 'AUD' }, note: 'kept' })
    await inputs[0].setValue('1,300.5')
    expect(updates()).toEqual([])
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ price: { amount: '1300.5', currency: 'AUD' }, note: 'kept' }])
  })

  it('reports a new amount in the pinned currency, and leaves the workspace currency to the server', async () => {
    const { wrapper, inputs, updates } = render({}, { currency: 'NZD' })
    // The workspace's code shows, but isn't sent: it may have changed.
    expect(wrapper.findAll('[data-testid="money-currency"]').map((c) => c.text())).toEqual(['NZD', 'USD'])
    await inputs[0].setValue('40')
    await inputs[0].trigger('keydown', { key: 'Enter' })
    await inputs[1].setValue('12.5')
    await inputs[1].trigger('keydown', { key: 'Enter' })
    expect(updates()).toEqual([{ price: { amount: '40' } }, { usd: { amount: '12.5', currency: 'USD' } }])
  })

  it('reports nothing for an amount that does not read, says why, and says it is invalid', async () => {
    const { wrapper, inputs, updates, invalid } = render({ price: { amount: '10.00', currency: 'AUD' } })
    await inputs[0].setValue('ten')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    expect(wrapper.text()).toContain('asset-attribute-money-invalid')
    expect(inputs[0].attributes('aria-invalid')).toBe('true')
    expect(invalid()).toEqual([true])
    await inputs[0].setValue('11')
    await inputs[0].trigger('blur')
    expect(invalid()).toEqual([true, false])
  })

  it('refuses more decimals than the currency has, on the field', async () => {
    const { wrapper, inputs, updates } = render({ price: { amount: '10.00', currency: 'AUD' } })
    await inputs[0].setValue('12.345')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    expect(wrapper.text()).toContain('asset-attribute-money-too-many-decimals')
  })

  it('drops the key when the amount is cleared, and reports nothing unchanged', async () => {
    const { inputs, updates } = render({ price: { amount: '10.00', currency: 'AUD' }, note: 'kept' })
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    await inputs[0].setValue('')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ note: 'kept' }])
  })

  it('reports each amount as it reads in a form with a Save button', async () => {
    const { wrapper, inputs, updates, invalid } = render({}, { eager: true })
    await inputs[1].setValue('12')
    expect(updates()).toEqual([{ usd: { amount: '12', currency: 'USD' } }])
    await inputs[1].setValue('-')
    // Half an amount holds the save rather than being dropped...
    expect(invalid()).toEqual([true])
    expect(updates()).toHaveLength(1)
    // ...but says nothing until the field is left or a save is tried.
    expect(wrapper.text()).not.toContain('asset-attribute-money-invalid')
    await wrapper.setProps({ showErrors: true })
    expect(wrapper.text()).toContain('asset-attribute-money-invalid')
  })

  it('says nothing halfway through an amount, and why once the field is left', async () => {
    const { wrapper, inputs, invalid } = render({}, { eager: true })
    await inputs[1].setValue('1,2')
    expect(invalid()).toEqual([true])
    expect(wrapper.text()).not.toContain('asset-attribute-money-invalid')
    await inputs[1].setValue('1,250')
    expect(invalid()).toEqual([true, false])
    await inputs[1].setValue('1,2')
    await inputs[1].trigger('blur')
    expect(wrapper.text()).toContain('asset-attribute-money-invalid')
  })

  it('leaves a bare amount from before the field held money until someone edits it', async () => {
    const { inputs, updates } = render({ price: '1250' })
    expect((inputs[0].element as HTMLInputElement).value).toBe('1250')
    await inputs[0].trigger('focus')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    await inputs[0].setValue('1300')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ price: { amount: '1300' } }])
  })

  it('shows text that was never an amount with its error, and never drops it on a focus and blur', async () => {
    const { wrapper, inputs, updates, invalid } = render({ price: 'N/A', note: 'kept' })
    expect((inputs[0].element as HTMLInputElement).value).toBe('N/A')
    expect(wrapper.text()).toContain('asset-attribute-money-invalid')
    expect(invalid()).toEqual([true])
    await inputs[0].trigger('focus')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([])
    // Someone decides: emptied, it goes.
    await inputs[0].setValue('')
    await inputs[0].trigger('blur')
    expect(updates()).toEqual([{ note: 'kept' }])
  })

  it('saves once when Enter is followed by a blur before the server answers', async () => {
    const { wrapper, inputs, updates } = render({})
    await inputs[0].trigger('focus')
    await inputs[0].setValue('12')
    await inputs[0].trigger('keydown', { key: 'Enter' })
    expect(updates()).toEqual([{ price: { amount: '12' } }])
    // The page applies the change at once, without the currency the
    // server will fill.
    await wrapper.setProps({ modelValue: { price: { amount: '12' } } })
    await inputs[0].trigger('blur')
    expect(updates()).toHaveLength(1)
  })
})
