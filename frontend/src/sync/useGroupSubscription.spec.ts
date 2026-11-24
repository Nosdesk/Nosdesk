/**
 * A view holds a sync group while it lives. Leaving starts a grace period
 * that ends in an unsubscribe; any view holding the group again within it
 * cancels that, and so does the runtime's tear-down.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'

const lifecycle = vi.hoisted(() => ({
  subscribe: vi.fn(async (_group: string) => {}),
  unsubscribe: vi.fn(async (_group: string) => {}),
}))
vi.mock('./lifecycle', () => lifecycle)

import {
  cancelGroupReleases,
  GROUP_RELEASE_GRACE_MS,
  useGroupSubscription,
} from './useGroupSubscription'

/** Mount a view holding `group`; `leave` unmounts it. */
function enter(group: Parameters<typeof useGroupSubscription>[0]) {
  const scope = effectScope()
  const held = scope.run(() => useGroupSubscription(group))!
  return { held, leave: () => scope.stop() }
}

beforeEach(() => {
  vi.useFakeTimers()
  lifecycle.subscribe.mockClear()
  lifecycle.unsubscribe.mockClear()
})

afterEach(() => {
  cancelGroupReleases()
  vi.useRealTimers()
})

describe('useGroupSubscription', () => {
  it('subscribes on entry and settles with the subscribe', async () => {
    let finish!: () => void
    lifecycle.subscribe.mockImplementationOnce(() => new Promise<void>((r) => (finish = r)))
    const { held, leave } = enter('ticket:9')
    expect(lifecycle.subscribe).toHaveBeenCalledWith('ticket:9')

    let settled = false
    void held.settled().then(() => (settled = true))
    await Promise.resolve()
    expect(settled).toBe(false)
    finish()
    await vi.waitFor(() => expect(settled).toBe(true))
    leave()
  })

  it('unsubscribes only once the grace has run out', () => {
    const { leave } = enter('ticket:9')
    leave()

    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS - 1)
    expect(lifecycle.unsubscribe).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(lifecycle.unsubscribe).toHaveBeenCalledExactlyOnceWith('ticket:9')
  })

  it('stays subscribed when the view is entered again within the grace', () => {
    enter('ticket:9').leave()
    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS - 1000)
    const again = enter('ticket:9')

    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS * 2)
    expect(lifecycle.unsubscribe).not.toHaveBeenCalled()

    // Leaving the second time starts a fresh grace.
    again.leave()
    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS)
    expect(lifecycle.unsubscribe).toHaveBeenCalledExactlyOnceWith('ticket:9')
  })

  it('waits for the last view holding the group', () => {
    const board = enter('project:7')
    const gantt = enter('project:7')
    board.leave()

    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS * 2)
    expect(lifecycle.unsubscribe).not.toHaveBeenCalled()
    gantt.leave()
    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS)
    expect(lifecycle.unsubscribe).toHaveBeenCalledExactlyOnceWith('project:7')
  })

  it('moves to the new group when the getter changes, and holds nothing for null', async () => {
    const id = ref<number | null>(9)
    const { leave } = enter(() => (id.value != null ? `ticket:${id.value}` : null))
    id.value = 10
    await nextTick()
    expect(lifecycle.subscribe).toHaveBeenLastCalledWith('ticket:10')

    id.value = null
    await nextTick()
    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS)
    expect(lifecycle.unsubscribe.mock.calls.map(([g]) => g).sort()).toEqual(['ticket:10', 'ticket:9'])
    expect(lifecycle.subscribe).toHaveBeenCalledTimes(2)
    leave()
  })

  it('follows the getter in settled() before its watcher has run', async () => {
    const id = ref(9)
    const { held, leave } = enter(() => `ticket:${id.value}`)
    id.value = 10
    await held.settled()
    expect(lifecycle.subscribe).toHaveBeenLastCalledWith('ticket:10')
    await nextTick()
    expect(lifecycle.subscribe).toHaveBeenCalledTimes(2)
    leave()
  })

  it('drops pending unsubscribes on tear-down, and releases taken before it do nothing', () => {
    enter('ticket:9').leave()
    const open = enter('ticket:10')
    cancelGroupReleases()

    open.leave()
    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS * 2)
    expect(lifecycle.unsubscribe).not.toHaveBeenCalled()
  })
})
