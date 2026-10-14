/**
 * `PUT /assets/{id}` ignores `status`, so a plugin patch carrying it used to
 * come back as a success with nothing changed. The bridge refuses it instead.
 *
 * `assets.query` pages and filters through `/assets/paginated`, and
 * `assets.kinds` hands out the kinds with their attribute schemas, both behind
 * `asset:read`.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const updateAsset = vi.fn(async (id: number, patch: object) => ({ id, ...patch }))
const getPaginatedAssets = vi.fn(async (params: { page: number; pageSize: number }) => ({
  data: [{ id: 1, name: 'Forklift' }],
  total: 41,
  page: params.page,
  pageSize: params.pageSize,
  totalPages: 3,
}))
const listKinds = vi.fn(async () => [
  {
    id: 3,
    slug: 'vehicle',
    label: 'Vehicle',
    description: null,
    icon: null,
    attribute_schema: { type: 'object', properties: { bay: { type: 'integer' } } },
    sort_order: 40,
    is_builtin: true,
    category: 'physical',
    created_at: '',
    updated_at: '',
    created_by: null,
  },
])

vi.mock('@/services/assetService', () => ({
  getAssetById: vi.fn(),
  getAssets: vi.fn(),
  getPaginatedAssets: (...args: [{ page: number; pageSize: number }]) =>
    getPaginatedAssets(...args),
  updateAsset: (...args: [number, object]) => updateAsset(...args),
}))
vi.mock('@nosdesk/core/services/assetKindsService', () => ({
  assetKindsService: { list: () => listKinds() },
}))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000003',
  name: 'asset-writer',
  consented_permissions: ['asset:write'],
  manifest: { permissions: ['asset:write'] },
} as unknown as Plugin

describe('plugin asset updates', () => {
  beforeEach(() => updateAsset.mockClear())

  it('refuses a patch that changes status', async () => {
    const err = await createPluginAPI(plugin)
      .assets.update(5, { name: 'Laptop', status: 'retired' })
      .catch((e: unknown) => e)
    const pe = asPluginApiError(err)
    expect(pe?.code).toBe('invalid')
    expect(pe?.message).toMatch(/asset lifecycle/)
    expect(updateAsset).not.toHaveBeenCalled()
  })

  it('still applies a patch without status', async () => {
    const asset = await createPluginAPI(plugin).assets.update(5, { name: 'Laptop' })
    expect(updateAsset).toHaveBeenCalledOnce()
    expect(asset).toMatchObject({ id: 5, name: 'Laptop' })
  })
})

const reader = {
  uuid: '0190a5b2-0000-7000-8000-000000000004',
  name: 'asset-reader',
  consented_permissions: ['asset:read'],
  manifest: { permissions: ['asset:read'] },
} as unknown as Plugin

describe('plugin asset reads', () => {
  beforeEach(() => {
    getPaginatedAssets.mockClear()
  })

  it('queries one filtered page, clamping the page size', async () => {
    const page = await createPluginAPI(reader).assets.query({
      page: 2,
      pageSize: 500,
      search: 'fork',
      kind: ['vehicle', 'equipment'],
      group: 7,
      status: 'in_service',
      sortBy: 'attr:bay',
      sortOrder: 'desc',
    })
    expect(getPaginatedAssets).toHaveBeenCalledOnce()
    expect(getPaginatedAssets.mock.calls[0][0]).toEqual({
      page: 2,
      pageSize: 100,
      search: 'fork',
      kind: 'vehicle,equipment',
      groups: '7',
      status: 'in_service',
      sortField: 'attr:bay',
      sortDirection: 'desc',
    })
    expect(page).toEqual({
      assets: [{ id: 1, name: 'Forklift' }],
      total: 41,
      page: 2,
      pageSize: 100,
      totalPages: 3,
    })
  })

  it('lists kinds with their attribute schemas', async () => {
    const kinds = await createPluginAPI(reader).assets.kinds()
    expect(kinds).toEqual([
      {
        id: 3,
        slug: 'vehicle',
        label: 'Vehicle',
        description: null,
        icon: null,
        category: 'physical',
        attributeSchema: { type: 'object', properties: { bay: { type: 'integer' } } },
        sortOrder: 40,
        isBuiltin: true,
      },
    ])
  })

  it('needs asset:read for both', async () => {
    const none = { ...reader, consented_permissions: [], manifest: { permissions: [] } } as unknown as Plugin
    const api = createPluginAPI(none)
    expect(asPluginApiError(await api.assets.query().catch((e: unknown) => e))?.code).toBe('denied')
    expect(asPluginApiError(await api.assets.kinds().catch((e: unknown) => e))?.code).toBe('denied')
    expect(getPaginatedAssets).not.toHaveBeenCalled()
  })
})
