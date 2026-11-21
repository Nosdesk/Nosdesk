// Workspace-wide asset choices.
import apiClient from '../apiClient'

/** What requesters see of the workspace's assets. Staff always see every asset. */
export type RequesterAssetVisibility = 'all' | 'none'

export interface AssetSettings {
  requester_visibility: RequesterAssetVisibility
  /** The workspace currency, an ISO 4217 code; null until an admin sets one. */
  currency: string | null
  /** The currency the workspace's default locale suggests while `currency` is unset. */
  suggested_currency: string | null
  /** The codes `currency` may be set to. */
  currencies: string[]
}

/** A change: a field left out keeps its value, and `currency: null` clears it. */
export interface AssetSettingsUpdate {
  requester_visibility?: RequesterAssetVisibility
  currency?: string | null
}

export const assetSettingsService = {
  async get(): Promise<AssetSettings> {
    const { data } = await apiClient.get<AssetSettings>('/admin/asset-settings')
    return data
  },
  async save(update: AssetSettingsUpdate): Promise<AssetSettings> {
    const { data } = await apiClient.put<AssetSettings>('/admin/asset-settings', update)
    return data
  },
}
