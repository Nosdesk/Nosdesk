// Workspace-wide asset choices.
import apiClient from '../apiClient'

/** What requesters see of the workspace's assets. Staff always see every asset. */
export type RequesterAssetVisibility = 'all' | 'none'

export interface AssetSettings {
  requester_visibility: RequesterAssetVisibility
}

export const assetSettingsService = {
  async get(): Promise<AssetSettings> {
    const { data } = await apiClient.get<AssetSettings>('/admin/asset-settings')
    return data
  },
  async save(settings: AssetSettings): Promise<AssetSettings> {
    const { data } = await apiClient.put<AssetSettings>('/admin/asset-settings', settings)
    return data
  },
}
