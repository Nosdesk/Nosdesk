import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { PushPermission } from '@/platform/pushNotifications'

const push = vi.hoisted(() => ({
  checkPushPermission: vi.fn(),
  enableNativePush: vi.fn(),
}))

vi.mock('@/platform/pushNotifications', () => ({
  supportsNativePush: () => true,
  checkPushPermission: push.checkPushPermission,
  enableNativePush: push.enableNativePush,
  openPushSettings: vi.fn(),
}))

vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'user-1' } }) }))
vi.mock('@/composables/useNotificationSSE', () => ({ requestNotificationPermission: vi.fn() }))

vi.mock('@nosdesk/core/services/notificationService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/notificationService')>()),
  getNotificationPreferences: vi.fn(async () => []),
  getInterruptHumanOnly: vi.fn(async () => false),
}))

import NotificationSettings from '@/components/settings/NotificationSettings.vue'

let wrapper: VueWrapper | null = null
let visibility: DocumentVisibilityState = 'visible'
Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility })

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  visibility = 'visible'
  vi.clearAllMocks()
})

/** Mount with permission reading `first`, then `next` on every later read. */
async function mountWith(first: PushPermission, next: PushPermission) {
  push.checkPushPermission.mockResolvedValueOnce(first).mockResolvedValue(next)
  const successes: string[] = []
  wrapper = mountWithProviders(NotificationSettings, { onSuccess: (m: string) => successes.push(m) })
  await flushPromises()
  return successes
}

/** Leave the app (for the device settings, say) and come back. */
async function returnToApp() {
  visibility = 'hidden'
  document.dispatchEvent(new Event('visibilitychange'))
  visibility = 'visible'
  document.dispatchEvent(new Event('visibilitychange'))
  await flushPromises()
}

describe('NotificationSettings native push', () => {
  it('registers the device when permission comes back granted from the device settings', async () => {
    push.enableNativePush.mockResolvedValue({ permission: 'granted', registered: true })
    const successes = await mountWith('denied', 'granted')
    expect(wrapper!.text()).toContain('settings-notifications-push-denied-description')

    await returnToApp()

    expect(push.enableNativePush).toHaveBeenCalledTimes(1)
    expect(successes).toEqual(['settings-notifications-push-enabled-success'])
    expect(wrapper!.text()).toContain('settings-notifications-push-granted-description')
  })

  it('shows permission turned off while away, without registering', async () => {
    await mountWith('granted', 'denied')
    await returnToApp()
    expect(push.enableNativePush).not.toHaveBeenCalled()
    expect(wrapper!.text()).toContain('settings-notifications-push-denied-description')
  })

  it('does not register again when permission was already granted', async () => {
    await mountWith('granted', 'granted')
    await returnToApp()
    expect(push.checkPushPermission).toHaveBeenCalledTimes(2)
    expect(push.enableNativePush).not.toHaveBeenCalled()
  })

  it('stops listening once the screen is gone', async () => {
    await mountWith('denied', 'granted')
    wrapper!.unmount()
    wrapper = null
    await returnToApp()
    expect(push.checkPushPermission).toHaveBeenCalledTimes(1)
    expect(push.enableNativePush).not.toHaveBeenCalled()
  })
})
