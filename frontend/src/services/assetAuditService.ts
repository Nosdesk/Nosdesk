import apiClient from './apiConfig'

/** One row of the asset audit ledger. Decimal fields are
 *  serialised as strings to avoid lossy f64 round-tripping.
 *  `delta` is signed: positive = found more than book, negative
 *  = missing. */
export interface AssetAudit {
  id: number
  asset_id: number
  counted_quantity: string
  previous_quantity: string
  delta: string
  notes: string | null
  recorded_by: string | null
  recorded_at: string
}

export interface RecordAuditBody {
  counted_quantity: string
  notes?: string | null
}

export const assetAuditService = {
  /** Record a physical-count audit. Backend sets
   *  assets.quantity = counted_quantity in the same
   *  transaction; expect an `asset.updated` SSE on success. */
  async record(assetId: number, body: RecordAuditBody): Promise<AssetAudit> {
    const { data } = await apiClient.post<AssetAudit>(`/assets/${assetId}/audit`, body)
    return data
  },

  async listForAsset(
    assetId: number,
    opts: { limit?: number; offset?: number } = {},
  ): Promise<AssetAudit[]> {
    const { data } = await apiClient.get<AssetAudit[]>(`/assets/${assetId}/audits`, {
      params: { limit: opts.limit, offset: opts.offset },
    })
    return data
  },
}
