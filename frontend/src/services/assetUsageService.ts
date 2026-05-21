import apiClient from './apiConfig'

/** One ledger row from the asset usage log. Decimal fields are
 *  serialised as strings to avoid lossy f64 round-tripping
 *  (NUMERIC(12,3) on the backend). Direction lives in
 *  `event_kind`: `'usage'` decremented the asset's quantity,
 *  `'restock'` incremented it. `quantity_used` is the magnitude
 *  either way. */
export interface AssetUsage {
  id: number
  asset_id: number
  ticket_id: number | null
  /** BigDecimal-as-string. */
  quantity_used: string
  unit: string
  recorded_by: string | null
  recorded_at: string
  notes: string | null
  event_kind: 'usage' | 'restock'
}

export interface RecordAssetUsageBody {
  /** Decimal-as-string. Backend rejects non-positive values. */
  quantity_used: string
  /** Optional. Omit for ad-hoc events (restock receipts,
   *  write-offs); set for ticket-driven usage. */
  ticket_id?: number | null
  notes?: string | null
  /** Direction. Defaults to 'usage' on the wire. */
  kind?: 'usage' | 'restock'
}

export const assetUsageService = {
  /** Record a usage event against a stock-tracked asset. The
   *  backend decrements `assets.quantity` in the same
   *  transaction; expect an `asset.updated` SSE on success. */
  async record(assetId: number, body: RecordAssetUsageBody): Promise<AssetUsage> {
    const { data } = await apiClient.post<AssetUsage>(`/assets/${assetId}/usage`, body)
    return data
  },

  /** Paginated history for an asset, newest first. */
  async listForAsset(
    assetId: number,
    opts: { limit?: number; offset?: number } = {},
  ): Promise<AssetUsage[]> {
    const { data } = await apiClient.get<AssetUsage[]>(`/assets/${assetId}/usage`, {
      params: { limit: opts.limit, offset: opts.offset },
    })
    return data
  },

  /** Every usage row tied to a ticket. Newest first. */
  async listForTicket(ticketId: number): Promise<AssetUsage[]> {
    const { data } = await apiClient.get<AssetUsage[]>(`/tickets/${ticketId}/asset-usage`)
    return data
  },
}
