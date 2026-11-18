/**
 * A plugin whose manifest lists no `subscriptions` may still subscribe to any
 * host event, and its frame warns once per load the first time it does.
 *
 * Lives here, like hostMessageGate.spec.ts, because the runtime package is the
 * one wired up with vitest and it bundles the SDK's `connectToHost`.
 */
import { describe, expect, it, vi } from 'vitest'
import { subscriptionsWarner, UNDECLARED_SUBSCRIPTIONS_WARNING } from '@nosdesk/plugin-sdk'

describe('subscriptionsWarner', () => {
  it('warns once per load when the manifest lists no subscriptions', () => {
    const warn = vi.fn()
    const beforeSubscribe = subscriptionsWarner(false, warn)
    beforeSubscribe()
    beforeSubscribe()
    beforeSubscribe()
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn).toHaveBeenCalledWith(UNDECLARED_SUBSCRIPTIONS_WARNING)
    expect(UNDECLARED_SUBSCRIPTIONS_WARNING).toMatch(/next plugin API major version/)
  })

  it('stays quiet when the manifest lists subscriptions', () => {
    const warn = vi.fn()
    const beforeSubscribe = subscriptionsWarner(true, warn)
    beforeSubscribe()
    expect(warn).not.toHaveBeenCalled()
  })
})
