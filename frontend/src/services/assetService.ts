import apiClient from './apiConfig';
import type { Asset, AssetFormData } from '@/types/asset';
import type { PaginationParams, PaginatedResponse } from '@/types/pagination';
import { logger } from '@/utils/logger';
import { RequestManager } from '@/utils/requestManager';

// Request cancellation manager instance
const requestManager = new RequestManager();

// Extended pagination params for devices
export interface AssetPaginationParams extends PaginationParams {
  /** Comma-separated lifecycle statuses (e.g. `in_service,in_repair`). */
  status?: string;
  warranty?: string;
  location?: string;
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
export type { PaginatedResponse } from '@/types/pagination';

/**
 * Pass-through: the backend response shape now matches the
 * frontend `Asset` type 1:1. The previous mapper invented
 * "legacy" fields (type/status/specs) and copied each column
 * by hand; both became net-negative once Pass B moved IT data
 * into the attributes JSONB.
 */
const transformDeviceResponse = (backendDevice: Asset): Asset => backendDevice;

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

/** Download workspace assets as CSV, honouring the same filters as
 *  the paginated list. Uses the authenticated API client so errors
 *  surface in-app instead of replacing the page with raw text. */
export async function downloadAssetsCsv(
  filters: Pick<AssetPaginationParams, 'search' | 'status' | 'warranty' | 'location' | 'lowStock'>,
): Promise<void> {
  try {
    const response = await apiClient.get('/assets/export', {
      params: assetExportParams(filters),
      responseType: 'blob',
    });
    const blob = response.data as Blob;
    const filename = filenameFromContentDisposition(
      response.headers['content-disposition'] as string | undefined,
      'assets-export.csv',
    );
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  } catch (error) {
    const err = error as { response?: { data?: Blob } };
    if (err.response?.data instanceof Blob) {
      const message = await messageFromErrorBlob(err.response.data);
      if (message) {
        throw new Error(message);
      }
    }
    logger.error('Failed to export assets CSV', { error });
    throw error;
  }
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
