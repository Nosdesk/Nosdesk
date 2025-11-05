import apiClient from './apiConfig';
import type { Device, DeviceFormData } from '@/types/device';

// Request cancellation manager
class RequestManager {
  private activeRequests = new Map<string, AbortController>();

  createRequest(key: string): AbortController {
    // Cancel any existing request with the same key
    this.cancelRequest(key);
    
    // Create new abort controller
    const controller = new AbortController();
    this.activeRequests.set(key, controller);
    
    return controller;
  }

  cancelRequest(key: string): void {
    const controller = this.activeRequests.get(key);
    if (controller) {
      controller.abort();
      this.activeRequests.delete(key);
    }
  }

  cancelAllRequests(): void {
    this.activeRequests.forEach(controller => controller.abort());
    this.activeRequests.clear();
  }
}

const requestManager = new RequestManager();

// Pagination interface
export interface PaginationParams {
  page: number;
  pageSize: number;
  sortField?: string;
  sortDirection?: 'asc' | 'desc';
  search?: string;
  type?: string;
  warranty?: string;
}

// Paginated response interface
export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

/**
 * Transform backend device response to frontend Device interface
 */
const transformDeviceResponse = (backendDevice: any): Device => {
  return {
    id: backendDevice.id,
    name: backendDevice.name,
    hostname: backendDevice.hostname,
    serial_number: backendDevice.serial_number,
    model: backendDevice.model,
    warranty_status: backendDevice.warranty_status,
    manufacturer: backendDevice.manufacturer,
    primary_user_uuid: backendDevice.primary_user_uuid,
    intune_device_id: backendDevice.intune_device_id,
    entra_device_id: backendDevice.entra_device_id,
    created_at: backendDevice.created_at,
    updated_at: backendDevice.updated_at,
    last_sync_time: backendDevice.last_sync_time,
    is_editable: backendDevice.is_editable ?? true,
    primary_user: backendDevice.primary_user,
    // Legacy fields for backward compatibility
    type: backendDevice.manufacturer || determineDeviceType(backendDevice.model),
    lastSeen: backendDevice.updated_at || new Date().toISOString(),
    status: 'online', // Default status
    assignedTo: backendDevice.primary_user?.name || null,
    specs: getDeviceSpecs(backendDevice.model)
  };
};

/**
 * Get all devices
 * @returns Promise<Device[]> - A promise that resolves to an array of devices
 */
export const getDevices = async (): Promise<Device[]> => {
  try {
    const response = await apiClient.get(`/devices`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    console.error('Error fetching devices:', error);
    throw error;
  }
};

// Get paginated devices
export const getPaginatedDevices = async (params: PaginationParams, requestKey: string = 'paginated-devices'): Promise<PaginatedResponse<Device>> => {
  try {
    // Create cancellable request
    const controller = requestManager.createRequest(requestKey);
    
    const response = await apiClient.get(`/devices/paginated`, { 
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
  } catch (error: any) {
    // Don't throw if request was cancelled
    if (error.name === 'AbortError' || error.name === 'CanceledError') {
      console.log('Request cancelled:', requestKey);
      throw new Error('REQUEST_CANCELLED');
    }
    console.error('Error fetching paginated devices:', error);
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
    const response = await apiClient.get(`/devices/${id}`);
    return transformDeviceResponse(response.data);
  } catch (error) {
    console.error(`Error fetching device with ID ${id}:`, error);
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
    console.error(`Error fetching device for ticket ID ${ticketId}:`, error);
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
    const response = await apiClient.get(`/users/${userUuid}/devices`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    console.error(`Error fetching devices for user ${userUuid}:`, error);
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
    const response = await apiClient.post(`/devices`, deviceData);
    return transformDeviceResponse(response.data);
  } catch (error) {
    console.error('Error creating device:', error);
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
    // Convert frontend Device to backend update format
    const backendDevice = {
      name: device.name,
      hostname: device.hostname,
      serial_number: device.serial_number,
      model: device.model,
      warranty_status: device.warranty_status,
      manufacturer: device.manufacturer,
      primary_user_uuid: device.primary_user_uuid,
      intune_device_id: device.intune_device_id,
      entra_device_id: device.entra_device_id
    };
    
    const response = await apiClient.put(`/devices/${id}`, backendDevice);
    return transformDeviceResponse(response.data);
  } catch (error) {
    console.error(`Error updating device with ID ${id}:`, error);
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
    await apiClient.delete(`/devices/${id}`);
  } catch (error) {
    console.error(`Error deleting device with ID ${id}:`, error);
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
    const response = await apiClient.post(`/devices/${id}/unmanage`);
    return transformDeviceResponse(response.data);
  } catch (error) {
    console.error(`Error unmanaging device with ID ${id}:`, error);
    throw error;
  }
};

/**
 * Helper function to determine device type based on model
 * @param model - The device model
 * @returns string - The device type
 */
const determineDeviceType = (model: string): string => {
  const modelLower = model.toLowerCase();
  
  if (modelLower.includes('macbook') || modelLower.includes('thinkpad') || modelLower.includes('xps')) {
    return 'Laptop';
  } else if (modelLower.includes('iphone') || modelLower.includes('pixel')) {
    return 'Mobile';
  } else if (modelLower.includes('ipad') || modelLower.includes('tab')) {
    return 'Tablet';
  } else if (modelLower.includes('imac') || modelLower.includes('desktop')) {
    return 'Desktop';
  } else {
    return 'Other';
  }
};

/**
 * Helper function to get device specs based on model
 * @param model - The device model
 * @returns object - The device specs
 */
const getDeviceSpecs = (model: string): Device['specs'] => {
  const modelLower = model.toLowerCase();
  
  if (modelLower.includes('macbook') && modelLower.includes('pro')) {
    return {
      cpu: 'Apple M1 Pro',
      memory: '16GB',
      storage: '512GB SSD',
      os: 'macOS 14.0'
    };
  } else if (modelLower.includes('thinkpad')) {
    return {
      cpu: 'Intel Core i7-1165G7',
      memory: '16GB',
      storage: '512GB SSD',
      os: 'Windows 11'
    };
  } else if (modelLower.includes('xps')) {
    return {
      cpu: 'Intel Core i7-11800H',
      memory: '32GB',
      storage: '1TB SSD',
      os: 'Windows 11'
    };
  } else {
    return {
      cpu: 'Unknown',
      memory: 'Unknown',
      storage: 'Unknown',
      os: 'Unknown'
    };
  }
};

// Cancel all active requests
export const cancelAllRequests = (): void => {
  requestManager.cancelAllRequests();
};

// Get devices for a specific user (prioritized devices)
export const getUserDevices = async (userUuid: string): Promise<Device[]> => {
  try {
    const response = await apiClient.get(`/users/${userUuid}/devices`);
    return response.data.map(transformDeviceResponse);
  } catch (error) {
    console.error('Error fetching user devices:', error);
    throw error;
  }
};

// Get paginated devices excluding specific IDs
export const getPaginatedDevicesExcluding = async (params: {
  page?: number;
  pageSize?: number;
  search?: string;
  excludeIds?: number[];
}): Promise<PaginatedResponse<Device>> => {
  try {
    const response = await apiClient.get(`/devices/paginated/excluding`, {
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
    console.error('Error fetching paginated devices:', error);
    throw error;
  }
}; 127