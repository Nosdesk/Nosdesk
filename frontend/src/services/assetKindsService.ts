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

  async update(id: number, body: UpdateAssetKindBody): Promise<AssetKind> {
    const { data } = await apiClient.put<AssetKind>(`/admin/asset-kinds/${id}`, body)
    return data
  },

  async delete(id: number): Promise<void> {
    await apiClient.delete(`/admin/asset-kinds/${id}`)
  },
}
