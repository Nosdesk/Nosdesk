import apiClient from '@nosdesk/core/apiClient';
import type { Asset, AssetFormData } from '@nosdesk/core/types/asset';
import type { PaginationParams, PaginatedResponse } from '@nosdesk/core/types/pagination';
import { logger } from '@nosdesk/core/utils/logger';
import { RequestManager } from '@nosdesk/core/utils/requestManager';

// Request cancellation manager instance
const requestManager = new RequestManager();

// Extended pagination params for devices
export interface AssetPaginationParams extends PaginationParams {
  /** Comma-separated lifecycle statuses (e.g. `in_service,in_repair`). */
  status?: string;
  warranty?: string;
  location?: string;
  /** Comma-separated native asset-group ids; matches assets in ANY of them. */
  groups?: string;
  /** Set to `'true'` to restrict the page to stock-tracked
   *  assets at or below their low-stock threshold. Backend
   *  treats anything else as off. */
  lowStock?: string;
}

export interface AssetLocationOption {
  location: string;
  asset_count: number;
}

/** Shared Pinia Colada cache key for the distinct-locations list. */
export const ASSET_LOCATIONS_QUERY_KEY = ['asset-locations'] as const;

// Re-export for backwards compatibility
export type { PaginatedResponse } from '@nosdesk/core/types/pagination';

/**
 * Pass-through: the backend response shape now matches the
 * frontend `Asset` type 1:1. The previous mapper invented
 * "legacy" fields (type/status/specs) and copied each column
 * by hand; both became net-negative once Pass B moved IT data
 * into the attributes JSONB.
 */
const transformDeviceResponse = (backendDevice: Asset): Asset => backendDevice;

/** An asset row tagged with the server-derived planning buckets. The
 *  inventory list groups by these when a planning lens is active.
 *  `compliance_state` is grouped from `attributes.compliance_state`. */
export type AssetGroupingRow = Asset & {
  /** 'windows' | 'macos' | 'linux' | 'ios' | 'android' | 'other'. */
  os_family: string;
  /** 'expired' | 'expiring_30d' | 'expiring_90d' | 'active' | 'unknown'. */
  warranty_window: string;
};

/** Fetch the complete filtered asset set (no pagination) tagged with
 *  planning buckets. Drives the inventory list's fleet-planning lenses,
 *  where counts and selection must cover the whole fleet, not just the
 *  rows scrolled into view. Accepts the same filter keys as the
 *  paginated list. */
export const getAssetGroupingDataset = async (filters: {
  search?: string;
  status?: string;
  warranty?: string;
  location?: string;
  lowStock?: string;
}): Promise<AssetGroupingRow[]> => {
  const params: Record<string, string> = {};
  if (filters.search) params.search = filters.search;
  if (filters.status) params.status = filters.status;
  if (filters.warranty) params.warranty = filters.warranty;
  if (filters.location) params.location = filters.location;
  if (filters.lowStock) params.lowStock = filters.lowStock;
  const response = await apiClient.get('/assets/grouping-dataset', { params });
  return response.data as AssetGroupingRow[];
};

/** Body for creating a rollout from a selected device group. */
export interface CreateRolloutBody {
  name: string;
  description?: string | null;
  workflow_state_id: number;
  priority?: string;
  asset_ids: number[];
}

export interface CreateRolloutResult {
  project_id: number;
  ticket_count: number;
}

/** Mint a rollout project with one ticket per selected device, each
 *  ticket linked to its asset. One server-side transaction. */
export const createAssetRollout = async (
  body: CreateRolloutBody,
): Promise<CreateRolloutResult> => {
  const response = await apiClient.post('/assets/rollouts', body);
  return response.data as CreateRolloutResult;
};

/**
 * Get all devices
 * @returns Promise<Asset[]> - A promise that resolves to an array of devices
 */
export const getAssets = async (): Promise<Asset[]> => {
  try {
    const response = await apiClient.get(`/assets`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    logger.error('Failed to fetch devices', { error });
    throw error;
  }
};

export const getAssetLocations = async (): Promise<AssetLocationOption[]> => {
  try {
    const response = await apiClient.get<AssetLocationOption[]>(`/assets/locations`);
    return response.data;
  } catch (error) {
    logger.error('Failed to fetch asset locations', { error });
    throw error;
  }
};

/** Build query params for CSV export (shared by the download helper). */
function assetExportParams(
  filters: Pick<AssetPaginationParams, 'search' | 'status' | 'warranty' | 'location' | 'lowStock'>,
): URLSearchParams {
  const params = new URLSearchParams({ format: 'csv' });
  if (filters.search) params.set('search', filters.search);
  if (filters.status) params.set('status', filters.status);
  if (filters.warranty) params.set('warranty', filters.warranty);
  if (filters.location) params.set('location', filters.location);
  if (filters.lowStock) params.set('lowStock', filters.lowStock);
  return params;
}

function filenameFromContentDisposition(
  header: string | undefined,
  fallback: string,
): string {
  if (!header) return fallback;
  const match = /filename\*?=(?:UTF-8''|")?([^";]+)/i.exec(header);
  const raw = match?.[1]?.trim();
  return raw ? decodeURIComponent(raw.replace(/^"|"$/g, '')) : fallback;
}

async function messageFromErrorBlob(blob: Blob): Promise<string | null> {
  const text = (await blob.text()).trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text) as { error?: string; message?: string };
    return parsed.error || parsed.message || text;
  } catch {
    return text;
  }
}

/** Shared blob-download: GET a CSV via the authenticated client and
 *  save it, surfacing server errors in-app instead of replacing the
 *  page with raw text. */
async function downloadCsv(
  url: string,
  params: URLSearchParams | undefined,
  fallbackName: string,
): Promise<void> {
  try {
    const response = await apiClient.get(url, { params, responseType: 'blob' });
    const blob = response.data as Blob;
    const filename = filenameFromContentDisposition(
      response.headers['content-disposition'] as string | undefined,
      fallbackName,
    );
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(link.href);
  } catch (error) {
    const err = error as { response?: { data?: Blob } };
    if (err.response?.data instanceof Blob) {
      const message = await messageFromErrorBlob(err.response.data);
      if (message) {
        throw new Error(message);
      }
    }
    logger.error('Failed to download asset CSV', { url, error });
    throw error;
  }
}

/** Download workspace assets as CSV, honouring the same filters as the
 *  paginated list. `scope: 'history'` exports the lifecycle event log
 *  (one row per transition) instead of the current-state snapshot. */
export async function downloadAssetsCsv(
  filters: Pick<AssetPaginationParams, 'search' | 'status' | 'warranty' | 'location' | 'lowStock'>,
  scope?: 'history',
): Promise<void> {
  const params = assetExportParams(filters);
  if (scope) params.set('scope', scope);
  await downloadCsv(
    '/assets/export',
    params,
    scope === 'history' ? 'asset-history.csv' : 'assets-export.csv',
  );
}

/** Download one asset's full lifecycle history as a CSV record card
 *  (offboarding / disposal / dispute evidence). */
export async function downloadAssetRecordCard(assetId: number): Promise<void> {
  await downloadCsv(`/assets/${assetId}/record-card`, undefined, `record-card-${assetId}.csv`);
}

// Get paginated assets
export const getPaginatedAssets = async (params: PaginationParams, requestKey: string = 'paginated-assets'): Promise<PaginatedResponse<Asset>> => {
  try {
    // Create cancellable request
    const controller = requestManager.createRequest(requestKey);
    
    const response = await apiClient.get(`/assets/paginated`, { 
      params,
      signal: controller.signal 
    });
    
    // Remove from active requests on success
    requestManager.cancelRequest(requestKey);
    
    return {
      data: response.data.data.map(transformDeviceResponse),
      total: response.data.total,
      page: response.data.page,
      pageSize: response.data.pageSize,
      totalPages: response.data.totalPages,
    };
  } catch (error) {
    // Don't throw if request was cancelled
    const errorWithName = error as { name?: string };
    if (errorWithName.name === 'AbortError' || errorWithName.name === 'CanceledError') {
      logger.debug('Request cancelled', { requestKey });
      throw new Error('REQUEST_CANCELLED');
    }
    logger.error('Failed to fetch paginated devices', { error, params });
    throw error;
  }
};

/**
 * Get a device by ID
 * @param id - The ID of the device to fetch
 * @returns Promise<Asset> - A promise that resolves to a device
 */
export const getAssetById = async (id: number | string): Promise<Asset> => {
  try {
    const response = await apiClient.get(`/assets/${id}`);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to fetch device by ID', { error, deviceId: id });
    throw error;
  }
};

/**
 * Get devices by ticket ID
 * @param ticketId - The ID of the ticket
 * @returns Promise<Asset | null> - A promise that resolves to a device or null
 */
export const getAssetByTicketId = async (ticketId: number): Promise<Asset | null> => {
  try {
    const response = await apiClient.get(`/tickets/${ticketId}/device`);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to fetch device for ticket', { error, ticketId });
    return null;
  }
};

/**
 * Get devices by user UUID
 * @param userUuid - The UUID of the user
 * @returns Promise<Asset[]> - A promise that resolves to an array of devices
 */
export const getAssetsByUser = async (userUuid: string): Promise<Asset[]> => {
  try {
    const response = await apiClient.get(`/users/${userUuid}/assets`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    logger.error('Failed to fetch devices for user', { error, userUuid });
    throw error;
  }
};

/**
 * Create a new device
 * @param deviceData - The device data to create
 * @returns Promise<Asset> - A promise that resolves to the created device
 */
export const createAsset = async (deviceData: AssetFormData): Promise<Asset> => {
  try {
    const response = await apiClient.post(`/assets`, deviceData);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to create device', { error, deviceData });
    throw error;
  }
};

/**
 * Mint an empty asset and return it. Mirrors `createEmptyTicket`:
 * creation is a one-click action that drops the user on the asset's
 * detail page to fill in name, type, and optional properties inline.
 * There is no separate create form.
 */
export const createEmptyAsset = async (): Promise<Asset> => {
  try {
    const response = await apiClient.post('/assets/empty');
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to create empty asset', { error });
    throw error;
  }
};

/**
 * Stamp a catalog model onto an asset. The backend copies the model's
 * manufacturer, model name, kind, and default specs onto the row (no
 * clobber) and links model_id, returning the updated asset.
 */
export const setAssetModel = async (assetId: number, modelId: number): Promise<Asset> => {
  const response = await apiClient.post(`/assets/${assetId}/model`, { model_id: modelId });
  return transformDeviceResponse(response.data);
};

/** Unlink the catalog model. The stamped manufacturer/model snapshot
 *  stays on the asset; it becomes a model-less one-off. */
export const clearAssetModel = async (assetId: number): Promise<Asset> => {
  const response = await apiClient.delete(`/assets/${assetId}/model`);
  return transformDeviceResponse(response.data);
};



/**
 * Update a device
 * @param id - The ID of the device to update
 * @param device - The updated device data
 * @returns Promise<Asset> - A promise that resolves to the updated device
 */
export const updateAsset = async (id: number, device: Partial<Asset>): Promise<Asset> => {
  try {
    // Forward the partial directly. Pass B removed the
    // hand-mapped column projection; the backend DeviceUpdate
    // accepts only the universal columns plus kind/attributes,
    // which is exactly the shape `Partial<Asset>` carries.
    const response = await apiClient.put(`/assets/${id}`, device);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to update device', { error, deviceId: id });
    throw error;
  }
};

/**
 * Delete a device
 * @param id - The ID of the device to delete
 * @returns Promise<void>
 */
export const deleteAsset = async (id: number): Promise<void> => {
  try {
    await apiClient.delete(`/assets/${id}`);
  } catch (error) {
    logger.error('Failed to delete device', { error, deviceId: id });
    throw error;
  }
};

/**
 * Unmanage a device (remove Intune/Entra IDs to make it editable)
 * @param id - The ID of the device to unmanage
 * @returns Promise<Asset> - The updated device
 */
export const unmanageAsset = async (id: number): Promise<Asset> => {
  try {
    const response = await apiClient.post(`/assets/${id}/unmanage`);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to unmanage device', { error, deviceId: id });
    throw error;
  }
};

// Cancel all active requests
export const cancelAllRequests = (): void => {
  requestManager.cancelAllRequests();
};

// Get paginated devices excluding specific IDs
export const getPaginatedAssetsExcluding = async (params: {
  page?: number;
  pageSize?: number;
  search?: string;
  excludeIds?: number[];
}): Promise<PaginatedResponse<Asset>> => {
  try {
    const response = await apiClient.get(`/assets/paginated/excluding`, {
      params: {
        page: params.page,
        pageSize: params.pageSize,
        search: params.search,
        excludeIds: params.excludeIds?.join(',')
      }
    });

    return {
      data: response.data.data.map(transformDeviceResponse),
      total: response.data.total,
      page: response.data.page,
      pageSize: response.data.pageSize,
      totalPages: response.data.totalPages,
    };
  } catch (error) {
    logger.error('Failed to fetch paginated devices excluding IDs', { error, params });
    throw error;
  }
};

// Bulk operations on devices (admin only)
export const bulkAction = async (request: { action: 'delete'; ids: number[] }): Promise<{ affected: number }> => {
  const response = await apiClient.post('/assets/bulk', request);
  return response.data;
};
