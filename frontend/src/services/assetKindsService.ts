import apiClient from './apiConfig'

/** Closed set, mirrored from the backend DB CHECK constraint
 *  on asset_kinds.category. Drives which IT-flavoured form
 *  fields and planner UI the frontend renders for a given
 *  kind. */
export type AssetKindCategory = 'it' | 'logical' | 'physical' | 'bulk' | 'generic'

export const ASSET_KIND_CATEGORIES: AssetKindCategory[] = [
  'it',
  'logical',
  'physical',
  'bulk',
  'generic',
]

/**
 * One row from the asset_kinds registry. The frontend uses this
 * directly as the picker option list and as the source-of-truth
 * for the attribute form schema; nothing here is locally derived
 * so admin edits to a kind propagate immediately to every form
 * that renders against it.
 */
export interface AssetKind {
  id: number
  slug: string
  label: string
  description: string | null
  icon: string | null
  /** Constrained JSON Schema subset; see
   *  `backend/src/services/assets/kinds.rs` for the validator. */
  attribute_schema: Record<string, unknown>
  sort_order: number
  is_builtin: boolean
  category: AssetKindCategory
  created_at: string
  updated_at: string
  created_by: string | null
}

export interface CreateAssetKindBody {
  slug: string
  label: string
  description?: string | null
  icon?: string | null
  attribute_schema?: Record<string, unknown>
  sort_order?: number
  category?: AssetKindCategory
}

export interface UpdateAssetKindBody {
  label?: string
  /** `null` clears the field; omit to leave unchanged. */
  description?: string | null
  icon?: string | null
  attribute_schema?: Record<string, unknown>
  sort_order?: number
  category?: AssetKindCategory
}

/** Shape of `GET /admin/asset-kinds/{id}/usage`. The admin list
 *  surfaces the count next to each row and the delete-confirm
 *  modal warns when it's non-zero. */
export interface AssetKindUsage {
  asset_count: number
}

export const assetKindsService = {
  async list(): Promise<AssetKind[]> {
    const { data } = await apiClient.get<AssetKind[]>('/admin/asset-kinds')
    return data
  },

  async get(id: number): Promise<AssetKind> {
    const { data } = await apiClient.get<AssetKind>(`/admin/asset-kinds/${id}`)
    return data
  },

  async create(body: CreateAssetKindBody): Promise<AssetKind> {
    const { data } = await apiClient.post<AssetKind>('/admin/asset-kinds', body)
    return data
  },

  async update(id: number, body: UpdateAssetKindBody, opts?: { force?: boolean }): Promise<AssetKind> {
    const { data } = await apiClient.put<AssetKind>(
      `/admin/asset-kinds/${id}`,
      body,
      opts?.force ? { params: { force: 'true' } } : undefined,
    )
    return data
  },

  async delete(id: number): Promise<void> {
    await apiClient.delete(`/admin/asset-kinds/${id}`)
  },

  async getUsage(id: number): Promise<AssetKindUsage> {
    const { data } = await apiClient.get<AssetKindUsage>(`/admin/asset-kinds/${id}/usage`)
    return data
  },
}

/**
 * Stable Pinia Colada cache key for the asset-kinds list. Shared
 * by the admin CRUD page and any consumer that needs to render a
 * picker against the registry (asset detail view, future ticket-
 * asset linker), so an admin save invalidates every open view
 * in one shot rather than each consumer re-fetching on mount.
 */
export const ASSET_KINDS_QUERY_KEY = ['asset-kinds'] as const
