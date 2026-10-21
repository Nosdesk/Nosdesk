import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The trial banner is the owner's alone and renders nothing at all for
// everyone else, so the content below it never moves. Copy is covered in
// utils/__tests__/trialNotice.spec.ts; this covers when it renders.

const CP = 'https://manage.example.com'

const { hosted, auth, workspaces, openExternalUrl } = await vi.hoisted(async () => {
  const { ref, reactive } = await import('vue')
  return {
    hosted: ref(true),
    auth: reactive({ user: { uuid: 'user-1' } as { uuid: string } | null }),
    workspaces: reactive({ activeWorkspace: null as Record<string, unknown> | null }),
    openExternalUrl: vi.fn(async (_url: string) => {}),
  }
})

vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  isHostedDeploymentRef: hosted,
  getControlPlaneUrl: () => CP,
}))
vi.mock('@/platform', () => ({ openExternalUrl }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/stores/myWorkspaces', () => ({ useMyWorkspacesStore: () => workspaces }))

import TrialReminderBanner from '@/components/TrialReminderBanner.vue'

const NOW = new Date(2026, 10, 2, 10, 30)
const endIn = (days: number) => new Date(2026, 10, 2 + days, 17).toISOString()

function ownerRow(billing?: Record<string, unknown>, role = 'owner') {
  return { workspace_id: 1, workspace_uuid: 'ws-1', slug: 'acme', name: 'Acme', role, billing }
}

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
async function mountBanner() {
  wrapper = mountWithProviders(TrialReminderBanner)
  await settle()
}
const banner = () => document.querySelector<HTMLElement>('[role="status"]')

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  hosted.value = true
  auth.user = { uuid: 'user-1' }
  workspaces.activeWorkspace = null
  localStorage.clear()
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('TrialReminderBanner renders nothing', () => {
  it.each([
    ['before the workspace row loads', () => (workspaces.activeWorkspace = null)],
    ['without billing', () => (workspaces.activeWorkspace = ownerRow())],
    ['on a paid plan', () => (workspaces.activeWorkspace = ownerRow({ status: 'active', trial_end: endIn(3), cancel_at: null }))],
    ['for an unknown status', () => (workspaces.activeWorkspace = ownerRow({ status: 'whatever', trial_end: endIn(3), cancel_at: null }))],
    ['with more than seven days left', () => (workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(8), cancel_at: null }))],
    ['for an admin', () => (workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(3), cancel_at: null }, 'admin'))],
    [
      'self-hosted',
      () => {
        hosted.value = false
        workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(3), cancel_at: null })
      },
    ],
  ])('%s', async (_label, arrange) => {
    arrange()
    await mountBanner()
    expect(banner()).toBeNull()
    // Not even an empty wrapper that would take up space.
    expect(document.querySelector('[data-test="trial-reminder"]')).toBeNull()
  })
})

describe('TrialReminderBanner shows', () => {
  it.each([
    [7, 'info', 'secondary', 'trial-notice-soon-title'],
    [3, 'warning', 'primary', 'trial-notice-imminent-title'],
    [0, 'warning', 'primary', 'trial-notice-imminent-title'],
  ] as const)('%i days out as %s', async (days, tone, variant, title) => {
    workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(days), cancel_at: null })
    await mountBanner()
    expect(banner()?.dataset.tone).toBe(tone)
    expect(banner()?.textContent).toContain(title)
    const billing = document.querySelector<HTMLButtonElement>('[data-test="trial-reminder-billing"]')
    expect(billing?.dataset.variant).toBe(variant)
  })

  it('a cancelled trial as neutral, with no escalation', async () => {
    workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(2), cancel_at: endIn(2) })
    await mountBanner()
    expect(banner()?.dataset.tone).toBe('neutral')
    expect(banner()?.textContent).toContain('trial-notice-cancelled-title')
    expect(document.querySelector<HTMLElement>('[data-test="trial-reminder-billing"]')?.dataset.variant).toBe('secondary')
  })

  it('opens the account billing page from Manage billing', async () => {
    workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(5), cancel_at: null })
    await mountBanner()
    const billing = document.querySelector<HTMLButtonElement>('[data-test="trial-reminder-billing"]')!
    expect(billing.getAttribute('aria-label')).toBe('trial-notice-manage-billing-aria')
    billing.click()
    await settle()
    expect(openExternalUrl).toHaveBeenCalledWith(`${CP}/account/billing`)
  })
})

describe('TrialReminderBanner dismissal', () => {
  it('hides, remembers, and moves focus to the page heading', async () => {
    const header = document.createElement('header')
    header.innerHTML = '<h1>Tickets</h1>'
    document.body.prepend(header)
    workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(5), cancel_at: null })
    await mountBanner()

    document.querySelector<HTMLButtonElement>('[data-test="trial-reminder-dismiss"]')!.click()
    await settle()

    expect(banner()).toBeNull()
    expect(document.activeElement?.tagName).toBe('H1')
    expect(localStorage.getItem(`nosdesk-trial-notice:ws-1:user-1:${endIn(5)}:soon`)).toBe('1')

    wrapper!.unmount()
    await mountBanner()
    expect(banner(), 'still dismissed after a remount').toBeNull()
  })

  it('falls back to the main landmark when the header has no plain heading', async () => {
    const main = document.createElement('main')
    document.body.appendChild(main)
    workspaces.activeWorkspace = ownerRow({ status: 'trialing', trial_end: endIn(2), cancel_at: null })
    await mountBanner()

    document.querySelector<HTMLButtonElement>('[data-test="trial-reminder-dismiss"]')!.click()
    await settle()

    expect(document.activeElement).toBe(main)
    expect(main.getAttribute('tabindex')).toBe('-1')
  })
})
