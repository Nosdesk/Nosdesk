import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, h, nextTick, ref } from 'vue'
import { mount } from '@vue/test-utils'

const lifecycle = vi.hoisted(() => ({
  subscribe: vi.fn(async () => {}),
  caughtUp: { value: false },
}))
vi.mock('@/sync/lifecycle', async () => {
  const { ref } = await import('vue')
  const r = ref(false)
  lifecycle.caughtUp = r
  return { subscribe: lifecycle.subscribe, caughtUp: r }
})
const workspaceId = ref<number | null>(7)
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspaceId: workspaceId }),
}))
vi.mock('pinia', async (orig) => ({
  ...(await orig<typeof import('pinia')>()),
  storeToRefs: (s: unknown) => s,
}))

import { useWorkspaceGroupSubscription } from '@/sync/useWorkspaceGroup'

function mountIt() {
  let out!: ReturnType<typeof useWorkspaceGroupSubscription>
  const C = defineComponent({
    setup() {
      out = useWorkspaceGroupSubscription()
      return () => h('div')
    },
  })
  const w = mount(C)
  return { w, out }
}

beforeEach(() => {
  setActivePinia(createPinia())
  lifecycle.subscribe.mockClear()
  lifecycle.caughtUp.value = false
})

// `ready` clears loading states whatever happened; `populated` says the
// pool reflects the server, which is what a first-run decision needs.
describe('useWorkspaceGroupSubscription', () => {
  it('is ready but not populated when the bootstrap did not complete', async () => {
    const { w, out } = mountIt()
    await nextTick()
    await nextTick()
    expect(lifecycle.subscribe).toHaveBeenCalledWith('workspace:7')
    expect(out.ready.value).toBe(true)
    expect(out.populated.value).toBe(false)
    w.unmount()
  })

  it('becomes populated when a deferred bootstrap completes later', async () => {
    const { w, out } = mountIt()
    await nextTick()
    await nextTick()
    expect(out.populated.value).toBe(false)
    lifecycle.caughtUp.value = true
    await nextTick()
    expect(out.populated.value).toBe(true)
    w.unmount()
  })

  it('is populated once the pool is caught up', async () => {
    lifecycle.caughtUp.value = true
    const { w, out } = mountIt()
    await nextTick()
    await nextTick()
    expect(out.ready.value).toBe(true)
    expect(out.populated.value).toBe(true)
    w.unmount()
  })
})
