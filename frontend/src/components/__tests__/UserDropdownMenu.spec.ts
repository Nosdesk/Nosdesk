import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// A Cloud trialist could not find billing anywhere in the app. On Cloud the
// workspace owner's menu links to the account dashboard's billing page, and
// everyone can reach support. Self-hosted shows neither.

const CP = 'https://manage.example.com'

const { hosted, auth, openExternalUrl } = await vi.hoisted(async () => ({
  hosted: (await import('vue')).ref(true),
  auth: {
    user: { uuid: 'u1', name: 'Olive Owner', email: 'o@example.com', avatar_url: null, workspace_role: 'owner' },
    isAdmin: true,
    logout: () => {},
  },
  openExternalUrl: vi.fn(async (_url: string) => {}),
}))

vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  isHostedDeploymentRef: hosted,
  getControlPlaneUrl: () => 'https://manage.example.com',
}))
vi.mock('@/platform', () => ({ openExternalUrl }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ showSwitcher: false, workspaces: [], activeWorkspaceId: null }),
}))
vi.mock('@/composables/useWorkspaceSwitch', () => ({ useWorkspaceSwitch: () => ({ switchWorkspace: vi.fn() }) }))
vi.mock('vue-router', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  useRouter: () => ({ push: vi.fn() }),
}))

import UserDropdownMenu from '@/components/UserDropdownMenu.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
beforeEach(() => {
  hosted.value = true
  auth.user.workspace_role = 'owner'
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function openMenu() {
  const button = document.createElement('button')
  document.body.appendChild(button)
  wrapper = mountWithProviders(UserDropdownMenu, { showMenu: true, buttonRef: button })
  await settle()
}

/** The menu row with this label, from the whole document (the menu teleports). */
function row(label: string): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((el) => el.textContent?.includes(label))
}

describe('Plan and billing', () => {
  it('opens the account billing page in a new tab for a Cloud owner', async () => {
    await openMenu()
    const billing = row('user-menu-plan-billing')
    expect(billing, 'the billing row').toBeTruthy()
    billing!.click()
    await settle()
    expect(openExternalUrl).toHaveBeenCalledWith(`${CP}/account/billing`)
  })

  it.each(['admin', 'agent'])('is not shown to a Cloud %s', async (role) => {
    auth.user.workspace_role = role
    await openMenu()
    expect(row('user-menu-plan-billing')).toBeUndefined()
    expect(row('user-menu-contact-support')).toBeTruthy()
  })

  it('is not shown to a self-hosted owner', async () => {
    hosted.value = false
    await openMenu()
    expect(row('user-menu-plan-billing')).toBeUndefined()
    expect(row('user-menu-team')).toBeTruthy()
  })
})

describe('Contact support', () => {
  it('shows the support address with a mailto link and a copy button on Cloud', async () => {
    auth.user.workspace_role = 'agent'
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    await openMenu()
    row('user-menu-contact-support')!.click()
    await settle()

    const mailto = document.querySelector<HTMLAnchorElement>('a[href="mailto:support@nosdesk.com"]')
    expect(mailto?.textContent?.trim()).toBe('support@nosdesk.com')
    const copy = document.querySelector<HTMLButtonElement>('button[aria-label="support-address-copy"]')
    expect(copy, 'the copy button').toBeTruthy()
    copy!.click()
    await settle()
    expect(writeText).toHaveBeenCalledWith('support@nosdesk.com')
  })

  it('is not shown on self-hosted', async () => {
    hosted.value = false
    await openMenu()
    expect(row('user-menu-contact-support')).toBeUndefined()
    expect(row('user-menu-report-problem')).toBeTruthy()
  })
})
