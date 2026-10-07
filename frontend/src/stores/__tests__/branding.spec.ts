import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// On the single-origin agent app the public branding route resolves no
// workspace (it goes by the Host), so a selected workspace's branding comes
// from the authenticated route, painted from its cache first.

// The store only reads `.value`; hoisted so the mock factory can use it.
const slug = vi.hoisted(() => ({ value: null as string | null }))
vi.mock('@/services/activeWorkspace', () => ({
  activeWorkspaceSlugRef: slug,
  activeWorkspaceSlug: () => slug.value,
}))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ currentTheme: 'system', setTheme: vi.fn() }),
}))

const brandingService = vi.hoisted(() => ({
  getBrandingConfig: vi.fn(),
  getWorkspaceBranding: vi.fn(),
}))
vi.mock('@nosdesk/core/services/brandingService', () => ({ default: brandingService }))

// Installs the in-memory localStorage shim the other specs rely on.
import '@/test/mountWithProviders'
import { useBrandingStore } from '@/stores/branding'

const branding = (primary_color: string | null) => ({
  app_name: 'Nosdesk',
  logo_url: null,
  logo_light_url: null,
  favicon_url: null,
  primary_color,
  updated_at: null,
})

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  slug.value = null
  brandingService.getBrandingConfig.mockReset()
  brandingService.getWorkspaceBranding.mockReset()
})

describe('branding on the single-origin agent app', () => {
  it('loads a selected workspace through the authenticated route', async () => {
    slug.value = 'mercury'
    brandingService.getWorkspaceBranding.mockResolvedValue(branding('#2563eb'))
    const store = useBrandingStore()

    await store.loadWorkspaceBranding()

    expect(store.primaryColor).toBe('#2563eb')
    expect(brandingService.getBrandingConfig).not.toHaveBeenCalled()
    expect(localStorage.getItem('nosdesk_branding_cache:mercury')).toContain('#2563eb')
  })

  it('paints the workspace cache before the fetch returns', async () => {
    localStorage.setItem('nosdesk_branding_cache:mercury', JSON.stringify(branding('#2563eb')))
    slug.value = 'mercury'
    let resolve!: (v: ReturnType<typeof branding>) => void
    brandingService.getWorkspaceBranding.mockReturnValue(new Promise((r) => (resolve = r)))
    const store = useBrandingStore()

    const load = store.loadWorkspaceBranding()
    expect(store.primaryColor).toBe('#2563eb')
    resolve(branding('#16a34a'))
    await load
    expect(store.primaryColor).toBe('#16a34a')
  })

  it('drops a result for a workspace no longer selected', async () => {
    slug.value = 'mercury'
    let resolve!: (v: ReturnType<typeof branding>) => void
    brandingService.getWorkspaceBranding.mockReturnValue(new Promise((r) => (resolve = r)))
    const store = useBrandingStore()

    const load = store.loadWorkspaceBranding()
    slug.value = 'venus'
    resolve(branding('#2563eb'))
    await load
    expect(store.primaryColor).toBeNull()
  })

  it('leaves the public route alone while a workspace is selected', async () => {
    slug.value = 'mercury'
    const store = useBrandingStore()
    await store.loadBranding()
    expect(brandingService.getBrandingConfig).not.toHaveBeenCalled()
  })

  it('drops a public result that lands after a workspace is selected', async () => {
    let resolve!: (v: ReturnType<typeof branding>) => void
    brandingService.getBrandingConfig.mockReturnValue(new Promise((r) => (resolve = r)))
    brandingService.getWorkspaceBranding.mockResolvedValue(branding('#2563eb'))
    const store = useBrandingStore()

    const publicLoad = store.loadBranding()
    slug.value = 'mercury'
    await store.loadWorkspaceBranding()
    resolve(branding(null))
    await publicLoad
    expect(store.primaryColor).toBe('#2563eb')
  })

  it('uses the public route with no workspace selected', async () => {
    brandingService.getBrandingConfig.mockResolvedValue(branding('#c32222'))
    const store = useBrandingStore()
    await store.loadBranding()
    expect(store.primaryColor).toBe('#c32222')
    expect(brandingService.getWorkspaceBranding).not.toHaveBeenCalled()
  })
})

// The store and its cache hold only what pages display, whatever it is
// handed: the admin settings view passes it the full admin settings.
describe('branding store holds only public fields', () => {
  const PUBLIC_KEYS = [
    'app_name',
    'favicon_url',
    'logo_light_url',
    'logo_url',
    'primary_color',
    'updated_at',
  ]
  const adminShaped = {
    ...branding('#2563eb'),
    signature_default: 'Jane Doe, IT, 555 0100',
    channel_auto_ack_enabled: true,
    channel_auto_ack_template: 'We have your message.',
    email_security_note_enabled: true,
    email_security_note_template: 'We never ask for your password.',
    guest_ticket_rate_limit_per_hour: 5,
    portal_share_by_domain: true,
  }

  it('stores only public fields when given admin settings', () => {
    const store = useBrandingStore()
    store.updateConfig(adminShaped)

    expect(Object.keys(store.config).sort()).toEqual(PUBLIC_KEYS)
    expect(store.primaryColor).toBe('#2563eb')
    const cached = JSON.parse(localStorage.getItem('nosdesk_branding_cache') ?? '{}')
    expect(Object.keys(cached).sort()).toEqual(PUBLIC_KEYS)
  })

  it('reads back only public fields from a cache holding more', () => {
    localStorage.setItem('nosdesk_branding_cache', JSON.stringify(adminShaped))
    const store = useBrandingStore()

    expect(Object.keys(store.config).sort()).toEqual(PUBLIC_KEYS)
    expect(store.primaryColor).toBe('#2563eb')
    // And the cache is rewritten without the rest.
    const cached = JSON.parse(localStorage.getItem('nosdesk_branding_cache') ?? '{}')
    expect(Object.keys(cached).sort()).toEqual(PUBLIC_KEYS)
  })
})
