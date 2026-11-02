import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, nextTick, ref } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'

const getAssetById = vi.hoisted(() => vi.fn(async (id: number) => ({ id, name: 'Laptop' })))
vi.mock('@/services/assetService', () => ({ getAssetById, updateAsset: vi.fn() }))

import { useAssetDetail } from '@/composables/useAssetDetail'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

function mountWith(id: ReturnType<typeof ref<number>>) {
  wrapper = mount(
    defineComponent({
      setup() {
        useAssetDetail(id as never)
        return () => null
      },
    }),
    { global: { plugins: [createPinia(), PiniaColada] } },
  )
}

describe('useAssetDetail', () => {
  it('loads the asset its route names', async () => {
    mountWith(ref(11))
    await flushPromises()
    expect(getAssetById).toHaveBeenCalledWith(11)
  })

  // Leaving the asset page (after deleting it, say), the route has no id
  // while the page is still on screen.
  it("doesn't ask for an asset when the route has no id", async () => {
    const id = ref(11)
    mountWith(id)
    await flushPromises()
    getAssetById.mockClear()

    id.value = Number(undefined)
    await nextTick()
    await flushPromises()
    expect(getAssetById).not.toHaveBeenCalled()
  })
})
