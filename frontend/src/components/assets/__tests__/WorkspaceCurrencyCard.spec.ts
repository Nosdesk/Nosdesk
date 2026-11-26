import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { AssetSettings } from '@nosdesk/core/services/assetSettingsService'

const data = ref<AssetSettings | undefined>()
const save = vi.fn()
const invalidateQueries = vi.fn()

vi.mock('@/queries/workspaceQuery', () => ({
  useWorkspaceQuery: () => ({ data }),
  useWorkspaceQueryCache: () => ({ invalidateQueries }),
}))
vi.mock('@nosdesk/core/services/assetSettingsService', () => ({
  assetSettingsService: { save: (...args: unknown[]) => save(...args) },
}))

import WorkspaceCurrencyCard from '../WorkspaceCurrencyCard.vue'

const settle = (ms = 30) => new Promise((r) => setTimeout(r, ms))

let wrapper: VueWrapper | null = null

function settings(currency: string | null): AssetSettings {
  return {
    requester_visibility: 'all',
    currency,
    suggested_currency: 'USD',
    currencies: ['AUD', 'EUR', 'USD'],
  }
}

async function pick(label: string) {
  await wrapper!.get('button').trigger('click')
  await settle()
  const row = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]')).find(
    (r) => r.textContent?.trim().startsWith(label),
  )
  expect(row, label).toBeDefined()
  row!.click()
  await settle()
}

beforeEach(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width'),
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
  Element.prototype.scrollIntoView = () => {}
  save.mockReset().mockResolvedValue(undefined)
  invalidateQueries.mockReset()
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

describe('WorkspaceCurrencyCard', () => {
  it('suggests the locale currency while unset, and sets one', async () => {
    data.value = settings(null)
    wrapper = mountWithProviders(WorkspaceCurrencyCard)
    await settle()
    expect(wrapper.text()).toContain('asset-currency-suggested-hint')
    expect(wrapper.get('button').text()).toContain('asset-currency-not-set')

    await pick('AUD')
    expect(save).toHaveBeenCalledWith({ currency: 'AUD' })
    expect(invalidateQueries).toHaveBeenCalled()
    // Cost and money fields read the currency from the record types.
    expect(invalidateQueries).toHaveBeenCalledWith({ key: ['asset-record-types'] })
    expect(wrapper.text()).toContain('asset-currency-set-hint')
  })

  it('clears the currency with Not set', async () => {
    data.value = settings('EUR')
    wrapper = mountWithProviders(WorkspaceCurrencyCard)
    await settle()
    expect(wrapper.get('button').text()).toContain('EUR')

    await pick('asset-currency-not-set')
    expect(save).toHaveBeenCalledWith({ currency: null })
  })
})
