import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { LicenseOverview } from '@nosdesk/core/types/license'

// Connecting needs Nosdesk Cloud to offer it. Where it doesn't yet, the page
// says so plainly instead of showing a failure.

const overview: LicenseOverview = {
  self_hosted: true,
  edition: 'community',
  max_workspaces: 1,
  active_workspaces: 1,
  can_create_workspace: false,
  instance_id: 'inst-1',
  license: {
    source: 'none',
    env_managed: false,
    error: null,
    installed_at: null,
    auto_refresh: true,
    last_refresh_at: null,
    last_refresh_error: null,
    details: null,
  },
  push: null,
  link: null,
}

const startLink = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/licenseService', () => ({
  default: { getOverview: async () => overview, startLink },
}))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({ getControlPlaneUrl: () => null }))

import LicenseCloudView from '@/views/admin/LicenseCloudView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

/** Reject as the API client does for a 502 carrying `code`. */
function cloudError(code: string) {
  return Object.assign(new Error(code), { response: { status: 502, data: { code } } })
}

async function connect() {
  wrapper = mountWithProviders(LicenseCloudView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  const button = wrapper.findAll('button').find((b) => b.text() === 'admin-license-connect')
  expect(button, 'the connect button').toBeTruthy()
  await button!.trigger('click')
  await flushPromises()
  return wrapper
}

describe('connecting to Nosdesk Cloud', () => {
  it('says when it is not available yet, without an error', async () => {
    startLink.mockRejectedValueOnce(cloudError('cloud_not_available'))
    const page = await connect()

    expect(page.text()).toContain('admin-license-connect-not-available')
    expect(page.text()).not.toContain('admin-license-connect-error-unexpected')
  })

  it('still reports a cloud it could not reach as an error', async () => {
    startLink.mockRejectedValueOnce(cloudError('cloud_unreachable'))
    const page = await connect()

    expect(page.text()).toContain('admin-license-connect-error-unreachable')
    expect(page.text()).not.toContain('admin-license-connect-not-available')
  })
})
