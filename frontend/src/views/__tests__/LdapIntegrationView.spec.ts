import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The real ldapService runs against a stubbed transport, so these tests also
// pin the PUT body the service builds (the backend's LdapSettingsRequest tests
// parse the same shape).
const api = vi.hoisted(() => ({
  settings: null as Record<string, unknown> | null,
  hasBindPassword: false,
  put: vi.fn(),
}))

const AD_PRESET = {
  id: 'active_directory',
  label: 'Active Directory',
  // serde_json without preserve_order: keys come back alphabetical.
  defaults: {
    attribute_map: { display_name: 'displayName', email: 'mail', external_id: 'objectGUID', first_name: 'givenName', last_name: 'sn' },
    group_config: { member_attribute: 'member', membership_mode: 'memberOf', object_class: 'group' },
    port: 636,
    tls_mode: 'ldaps',
    user_filter: '(&(objectCategory=person)(objectClass=user)(sAMAccountName={username}))',
    username_attribute: 'sAMAccountName',
  },
}

vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    get: async (url: string) => {
      if (url === '/ldap/settings') {
        return { data: { settings: api.settings, has_bind_password: api.hasBindPassword } }
      }
      if (url === '/ldap/presets') return { data: [AD_PRESET] }
      if (url === '/ldap/sync-history') {
        return { data: { runs: [], cursor: { incremental_active: false, last_full_reconcile_at: null } } }
      }
      throw new Error(`unexpected GET ${url}`)
    },
    put: (...args: unknown[]) => api.put(...args),
    post: vi.fn(),
  },
}))
// Dropdowns reduced to one button per option.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue', 'label'],
    emits: ['update:modelValue'],
    template: `<div data-test="dropdown" :data-label="label"><button v-for="o in options" :key="o.value" type="button" :data-value="o.value" @click="$emit('update:modelValue', o.value)">{{ o.label }}</button></div>`,
  },
}))
vi.mock('@/components/common/SearchableDropdown.vue', () => ({
  default: { props: ['modelValue'], template: '<div />' },
}))

import LdapIntegrationView from '@/views/LdapIntegrationView.vue'

/** An existing AD config as the server returns it: jsonb hands keys back
 *  shortest first, not in the order the preset or the form wrote them. */
function savedAdConfig(): Record<string, unknown> {
  return {
    workspace_id: 1,
    enabled: true,
    host: 'dc01.acme.test',
    port: 636,
    tls_mode: 'ldaps',
    verify_certs: true,
    ca_cert_pem: null,
    follow_referrals: false,
    connect_timeout_secs: 10,
    auth_mode: 'simple_bind',
    bind_dn: 'CN=svc-nosdesk,OU=Service Accounts,DC=acme,DC=test',
    user_base_dn: 'OU=Staff,DC=acme,DC=test',
    username_attribute: 'sAMAccountName',
    user_filter: AD_PRESET.defaults.user_filter,
    page_size: 500,
    attribute_map: { email: 'mail', last_name: 'sn', first_name: 'givenName', external_id: 'objectGUID', display_name: 'displayName' },
    group_config: { object_class: 'group', membership_mode: 'memberOf', member_attribute: 'member' },
    provisioning: {},
  }
}

let wrapper: VueWrapper | null = null

beforeEach(() => {
  api.settings = null
  api.hasBindPassword = false
  api.put.mockReset()
  api.put.mockImplementation(async (_url: string, body: { settings: Record<string, unknown> }) => ({
    data: { settings: { workspace_id: 1, ...body.settings }, has_bind_password: true },
  }))
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountView() {
  wrapper = mountWithProviders(LdapIntegrationView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

/** The control a `<label for>` with this text points at (translations resolve
 *  to the message id here; required fields append an asterisk). */
function control(key: string) {
  const label = wrapper!.findAll('label').find((l) => l.text().replace('*', '').trim() === key)
  expect(label, key).toBeTruthy()
  return wrapper!.find(`[id="${label!.attributes('for')}"]`)
}

function saveButton() {
  const btn = wrapper!.findAll('button').find((b) => b.text() === 'admin-ldap-save')
  expect(btn).toBeTruthy()
  return btn!
}

describe('LdapIntegrationView first-time setup', () => {
  it('explains why Save is off while an enabled config has no host', async () => {
    await mountView()
    await control('admin-ldap-enabled-label').trigger('click')
    expect(saveButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.find('[data-test="ldap-save-blocked"]').text()).toBe('admin-ldap-save-needs-host')

    await control('admin-ldap-host-label').setValue('dc01.acme.test')
    expect(wrapper!.find('[data-test="ldap-save-blocked"]').text()).toBe('admin-ldap-save-needs-base-dn')
  })

  it('saves an Active Directory config from the preset', async () => {
    await mountView()
    await control('admin-ldap-enabled-label').trigger('click')
    await wrapper!.find('[data-value="active_directory"]').trigger('click')
    await control('admin-ldap-host-label').setValue('dc01.acme.test')
    await control('admin-ldap-binddn-label').setValue('CN=svc-nosdesk,OU=Service Accounts,DC=acme,DC=test')
    await wrapper!.find('input[type="password"]').setValue('s3cret')
    await control('admin-ldap-userbase-label').setValue('OU=Staff,DC=acme,DC=test')

    expect(wrapper!.find('[data-test="ldap-save-blocked"]').exists()).toBe(false)
    expect(saveButton().attributes('disabled')).toBeUndefined()
    await saveButton().trigger('click')
    await flushPromises()

    expect(api.put).toHaveBeenCalledTimes(1)
    const [url, body] = api.put.mock.calls[0]!
    expect(url).toBe('/ldap/settings')
    expect(body).toEqual({
      settings: expect.objectContaining({
        enabled: true,
        host: 'dc01.acme.test',
        port: 636,
        tls_mode: 'ldaps',
        connect_timeout_secs: 10,
        page_size: 500,
        user_base_dn: 'OU=Staff,DC=acme,DC=test',
        user_filter: AD_PRESET.defaults.user_filter,
        attribute_map: AD_PRESET.defaults.attribute_map,
        group_config: AD_PRESET.defaults.group_config,
        provisioning: {},
      }),
      bind_password: 's3cret',
      clear_bind_password: false,
    })
    expect(wrapper!.find('[data-test="ldap-action-error"]').exists()).toBe(false)
    // Saved and reseeded from the response: nothing left to save.
    expect(saveButton().attributes('disabled')).toBeDefined()
  })

  it('shows a rejected save next to the Save button', async () => {
    api.put.mockRejectedValue({
      response: {
        status: 422,
        data: { error: 'user_filter must contain the {username} placeholder' },
        config: { url: '/ldap/settings' },
      },
    })
    await mountView()
    await control('admin-ldap-host-label').setValue('dc01.acme.test')
    await saveButton().trigger('click')
    await flushPromises()

    const error = wrapper!.find('[data-test="ldap-action-error"]')
    expect(error.text()).toContain('user_filter must contain the {username} placeholder')
    // Rendered in the sticky action bar, alongside the button.
    expect(error.element.parentElement).toBe(saveButton().element.closest('.sticky'))

    // Editing again clears the stale error.
    await control('admin-ldap-host-label').setValue('dc02.acme.test')
    expect(wrapper!.find('[data-test="ldap-action-error"]').exists()).toBe(false)
  })
})

describe('LdapIntegrationView editing a saved config', () => {
  beforeEach(() => {
    api.settings = savedAdConfig()
    api.hasBindPassword = true
  })

  it('is clean on load and after re-picking the same preset', async () => {
    await mountView()
    expect(saveButton().attributes('disabled')).toBeDefined()

    // Same values, different key order from the server's jsonb.
    await wrapper!.find('[data-value="active_directory"]').trigger('click')
    expect(saveButton().attributes('disabled')).toBeDefined()
  })

  it('saves an edit without touching the stored password', async () => {
    await mountView()
    await control('admin-ldap-host-label').setValue('dc02.acme.test')
    expect(saveButton().attributes('disabled')).toBeUndefined()
    await saveButton().trigger('click')
    await flushPromises()

    const [, body] = api.put.mock.calls[0]!
    expect(body.settings.host).toBe('dc02.acme.test')
    expect(body.settings.attribute_map).toEqual(savedAdConfig().attribute_map)
    expect(body.bind_password).toBeUndefined()
    expect(body.clear_bind_password).toBe(false)
  })
})
