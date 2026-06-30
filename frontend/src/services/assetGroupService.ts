import apiClient from '@nosdesk/core/apiClient';
import type { AssetGroup } from '@nosdesk/core/types/asset';
import { logger } from '@nosdesk/core/utils/logger';

/** A group definition plus its current member count (the `GET /asset-groups`
 *  row). Richer than the compact `AssetGroup` ref carried on an asset. */
export interface AssetGroupSummary {
  id: number;
  uuid: string;
  name: string;
  description?: string | null;
  color?: string | null;
  display_order: number;
  archived_at?: string | null;
  asset_count: number;
}

export interface AssetGroupInput {
  name: string;
  description?: string | null;
  color?: string | null;
  display_order?: number;
}

/** Pinia Colada key for the management list (all groups, incl. archived). */
export const ASSET_GROUPS_ALL_QUERY_KEY = ['asset-groups', 'all'] as const;

export const listAssetGroups = async (includeArchived = false): Promise<AssetGroupSummary[]> => {
  const response = await apiClient.get('/asset-groups', {
    params: includeArchived ? { include_archived: true } : undefined,
  });
  return response.data as AssetGroupSummary[];
};

export const createAssetGroup = async (input: AssetGroupInput): Promise<AssetGroupSummary> => {
  const response = await apiClient.post('/asset-groups', input);
  return response.data as AssetGroupSummary;
};

export const updateAssetGroup = async (
  id: number,
  input: Partial<AssetGroupInput>,
): Promise<AssetGroupSummary> => {
  const response = await apiClient.put(`/asset-groups/${id}`, input);
  return response.data as AssetGroupSummary;
};

export const archiveAssetGroup = async (id: number): Promise<AssetGroupSummary> => {
  const response = await apiClient.post(`/asset-groups/${id}/archive`);
  return response.data as AssetGroupSummary;
};

export const restoreAssetGroup = async (id: number): Promise<AssetGroupSummary> => {
  const response = await apiClient.post(`/asset-groups/${id}/restore`);
  return response.data as AssetGroupSummary;
};

/** Replace an asset's native group set (assigned from the asset side). Returns
 *  the resulting group refs so the caller can render them without rebuilding. */
export const setAssetGroupsForAsset = async (
  assetId: number,
  groupIds: number[],
): Promise<AssetGroup[]> => {
  try {
    const response = await apiClient.put(`/assets/${assetId}/groups`, { group_ids: groupIds });
    return (response.data ?? []) as AssetGroup[];
  } catch (error) {
    logger.error('Failed to set asset groups', { error, assetId });
    throw error;
  }
};
