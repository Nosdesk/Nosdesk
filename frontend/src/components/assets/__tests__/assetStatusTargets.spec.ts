import { afterEach, describe, expect, it, vi } from 'vitest'

// The status catalogue from the server, and the targets the status dialog
// offers from it: the server's moves when it sends them, the old client rule
// when it answers 404 (a 1.2 app on a 1.1 server).

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@nosdesk/core/apiClient', () => ({ default: api }))

const { assetStatusService, manualTargets } = await import(
  '@nosdesk/core/services/assetStatusService'
)
type Catalogue = NonNullable<Awaited<ReturnType<typeof assetStatusService.list>>>

const catalogue: Catalogue = {
  categories: [
    {
      key: 'in_service',
      default: true,
      start: true,
      loanable: true,
      assignable: true,
      old_asset_target: false,
      terminal: false,
      targets: [
        { key: 'in_stock', needs: [] },
        { key: 'on_loan', needs: ['active_loan'] },
        { key: 'disposed', needs: ['disposal'] },
      ],
    },
    {
      key: 'on_loan',
      default: false,
      start: false,
      loanable: false,
      assignable: false,
      old_asset_target: false,
      terminal: false,
      targets: [
        { key: 'in_service', needs: ['no_active_loan'] },
        { key: 'lost', needs: ['no_active_loan'] },
      ],
    },
  ],
  statuses: [],
}

afterEach(() => vi.clearAllMocks())

describe('assetStatusService.list', () => {
  it('returns the catalogue', async () => {
    api.get.mockResolvedValue({ data: catalogue })
    expect(await assetStatusService.list()).toEqual(catalogue)
    expect(api.get).toHaveBeenCalledWith('/assets/statuses')
  })

  it('returns null from a server without the endpoint', async () => {
    api.get.mockRejectedValue({ response: { status: 404 } })
    expect(await assetStatusService.list()).toBeNull()
  })

  it('passes any other failure on', async () => {
    api.get.mockRejectedValue({ response: { status: 500 } })
    await expect(assetStatusService.list()).rejects.toEqual({ response: { status: 500 } })
  })
})

describe('manualTargets', () => {
  it('reads the server targets and what each needs', () => {
    expect(manualTargets(catalogue, 'in_service', false)).toEqual(['in_stock', 'disposed'])
    expect(manualTargets(catalogue, 'in_service', true)).toEqual(['in_stock', 'on_loan', 'disposed'])
    // Off on loan only once the loan is gone.
    expect(manualTargets(catalogue, 'on_loan', true)).toEqual([])
    expect(manualTargets(catalogue, 'on_loan', false)).toEqual(['in_service', 'lost'])
  })

  it('falls back to every other status on a 404, on loan only to restore an open loan', () => {
    expect(manualTargets(null, 'in_service', false)).toEqual([
      'in_stock',
      'on_order',
      'in_transit',
      'in_repair',
      'retired',
      'lost',
      'disposed',
    ])
    expect(manualTargets(null, 'in_service', true)).toContain('on_loan')
  })

  it('moves a status outside the nine as an in-service asset, in service included', () => {
    expect(manualTargets(catalogue, 'broken', false)).toEqual(['in_service', 'in_stock', 'disposed'])
  })

  it('falls back when the catalogue carries no targets (a requester)', () => {
    const plain: Catalogue = {
      ...catalogue,
      categories: catalogue.categories.map((c) => ({ ...c, targets: undefined })),
    }
    expect(manualTargets(plain, 'in_service', false)).toHaveLength(7)
  })
})
