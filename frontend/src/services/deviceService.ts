import apiClient from './apiConfig';
import type { Device, DeviceFormData } from '@/types/device';
import type { PaginationParams, PaginatedResponse } from '@/types/pagination';
import { logger } from '@/utils/logger';
import { RequestManager } from '@/utils/requestManager';

// Request cancellation manager instance
const requestManager = new RequestManager();

// Extended pagination params for devices
export interface DevicePaginationParams extends PaginationParams {
  type?: string;
  warranty?: string;
}

// Re-export for backwards compatibility
export type { PaginatedResponse } from '@/types/pagination';

/**
 * Pass-through: the backend response shape now matches the
 * frontend `Device` type 1:1. The previous mapper invented
 * "legacy" fields (type/status/specs) and copied each column
 * by hand; both became net-negative once Pass B moved IT data
 * into the attributes JSONB.
 */
const transformDeviceResponse = (backendDevice: Device): Device => backendDevice;

/**
 * Get all devices
 * @returns Promise<Device[]> - A promise that resolves to an array of devices
 */
export const getDevices = async (): Promise<Device[]> => {
  try {
    const response = await apiClient.get(`/assets`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    logger.error('Failed to fetch devices', { error });
    throw error;
  }
};

// Get paginated devices
export const getPaginatedDevices = async (params: PaginationParams, requestKey: string = 'paginated-devices'): Promise<PaginatedResponse<Device>> => {
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
 * @returns Promise<Device> - A promise that resolves to a device
 */
export const getDeviceById = async (id: number | string): Promise<Device> => {
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
 * @returns Promise<Device | null> - A promise that resolves to a device or null
 */
export const getDeviceByTicketId = async (ticketId: number): Promise<Device | null> => {
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
 * @returns Promise<Device[]> - A promise that resolves to an array of devices
 */
export const getDevicesByUser = async (userUuid: string): Promise<Device[]> => {
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
 * @returns Promise<Device> - A promise that resolves to the created device
 */
export const createDevice = async (deviceData: DeviceFormData): Promise<Device> => {
  try {
    const response = await apiClient.post(`/assets`, deviceData);
    return transformDeviceResponse(response.data);
  } catch (error) {
    logger.error('Failed to create device', { error, deviceData });
    throw error;
  }
};



/**
 * Update a device
 * @param id - The ID of the device to update
 * @param device - The updated device data
 * @returns Promise<Device> - A promise that resolves to the updated device
 */
export const updateDevice = async (id: number, device: Partial<Device>): Promise<Device> => {
  try {
    // Forward the partial directly. Pass B removed the
    // hand-mapped column projection; the backend DeviceUpdate
    // accepts only the universal columns plus kind/attributes,
    // which is exactly the shape `Partial<Device>` carries.
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
export const deleteDevice = async (id: number): Promise<void> => {
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
 * @returns Promise<Device> - The updated device
 */
export const unmanageDevice = async (id: number): Promise<Device> => {
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
export const getPaginatedDevicesExcluding = async (params: {
  page?: number;
  pageSize?: number;
  search?: string;
  excludeIds?: number[];
}): Promise<PaginatedResponse<Device>> => {
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