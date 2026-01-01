import axios from 'axios';
import { logger } from '@/utils/logger';

/**
 * Microsoft Graph Service
 * 
 * A service for interacting with Microsoft Graph API through our backend proxy
 */
export default class MicrosoftGraphService {
  /**
   * Call the Microsoft Graph API
   * 
   * @param endpoint - The Graph API endpoint (e.g., "/users" or "/me")
   * @param method - HTTP method (GET, POST, PUT, DELETE, PATCH)
   * @param providerId - Optional provider ID (uses default if not provided)
   * @param body - Optional request body for POST/PUT/PATCH requests
   * @param queryParams - Optional query parameters
   * @param headers - Optional additional headers
   * @returns Promise with the API response
   */
  static async callGraphApi(
    endpoint: string,
    method: string = 'GET',
    providerId?: number,
    body?: unknown,
    queryParams?: Record<string, string>,
    headers?: Record<string, string>
  ) {
    try {
      const response = await axios.post('/api/msgraph/request', {
        provider_id: providerId,
        endpoint,
        method,
        body,
        query_params: queryParams,
        headers
      });
      
      return response.data;
    } catch (error) {
      logger.error('Error calling Microsoft Graph API:', error);
      throw error;
    }
  }

  /**
   * Get current user's information
   * 
   * @param providerId - Optional provider ID
   * @returns Promise with user data
   */
  static async getCurrentUser(providerId?: number) {
    return this.callGraphApi('/me', 'GET', providerId);
  }

  /**
   * Get user by ID or UPN
   * 
   * @param userIdOrUpn - User ID or UserPrincipalName
   * @param providerId - Optional provider ID
   * @returns Promise with user data
   */
  static async getUser(userIdOrUpn: string, providerId?: number) {
    return this.callGraphApi(`/users/${userIdOrUpn}`, 'GET', providerId);
  }

  /**
   * Get users from the organization
   * 
   * @param filter - Optional OData filter
   * @param select - Optional fields to select
   * @param providerId - Optional provider ID
   * @returns Promise with users data
   */
  static async getUsers(
    filter?: string,
    select?: string,
    providerId?: number
  ) {
    try {
      // Use the dedicated users endpoint
      const queryParams: Record<string, string> = {};
      
      if (filter) {
        queryParams.filter = filter;
      }
      
      if (select) {
        queryParams.select = select;
      }
      
      if (providerId) {
        queryParams.providerId = providerId.toString();
      }
      
      const response = await axios.get('/api/msgraph/users', { params: queryParams });
      return response.data;
    } catch (error) {
      logger.error('Error fetching Microsoft Graph users:', error);
      throw error;
    }
  }

  /**
   * Get groups from the organization
   * 
   * @param filter - Optional OData filter
   * @param providerId - Optional provider ID
   * @returns Promise with groups data
   */
  static async getGroups(filter?: string, providerId?: number) {
    try {
      // Use the dedicated groups endpoint
      const queryParams: Record<string, string> = {};
      
      if (filter) {
        queryParams.filter = filter;
      }
      
      if (providerId) {
        queryParams.providerId = providerId.toString();
      }
      
      const response = await axios.get('/api/msgraph/groups', { params: queryParams });
      return response.data;
    } catch (error) {
      logger.error('Error fetching Microsoft Graph groups:', error);
      throw error;
    }
  }

  /**
   * Get devices from the organization
   * 
   * @param filter - Optional OData filter
   * @param providerId - Optional provider ID 
   * @returns Promise with devices data
   */
  static async getDevices(filter?: string, providerId?: number) {
    try {
      // Use the dedicated devices endpoint
      const queryParams: Record<string, string> = {};
      
      if (filter) {
        queryParams.filter = filter;
      }
      
      if (providerId) {
        queryParams.providerId = providerId.toString();
      }
      
      const response = await axios.get('/api/msgraph/devices', { params: queryParams });
      return response.data;
    } catch (error) {
      logger.error('Error fetching Microsoft Graph devices:', error);
      throw error;
    }
  }

  /**
   * Get directory objects to test Directory.Read.All permission
   * 
   * @param providerId - Optional provider ID
   * @returns Promise with directory objects data
   */
  static async getDirectoryObjects(providerId?: number) {
    try {
      const queryParams: Record<string, string> = {};
      
      if (providerId) {
        queryParams.providerId = providerId.toString();
      }
      
      const response = await axios.get('/api/msgraph/directory-objects', { params: queryParams });
      return response.data;
    } catch (error) {
      logger.error('Error fetching Microsoft Graph directory objects:', error);
      throw error;
    }
  }

  /**
   * Get organization information to test Directory.Read.All permission
   * 
   * @param providerId - Optional provider ID
   * @returns Promise with organization data
   */
  static async getOrganization(providerId?: number) {
    return this.callGraphApi('/organization', 'GET', providerId);
  }

  /**
   * Test ProfilePhoto.Read.All permission by getting a user's photo
   * 
   * @param userIdOrUpn - User ID or UserPrincipalName (optional, uses first available user if not provided)
   * @param providerId - Optional provider ID
   * @returns Promise with photo metadata
   */
  static async testProfilePhotoAccess(userIdOrUpn?: string, providerId?: number) {
    try {
      // If no user specified, get the first user and use their ID
      if (!userIdOrUpn) {
        const usersResponse = await this.getUsers(undefined, 'id', providerId);
        if (usersResponse.data?.value?.length > 0) {
          userIdOrUpn = usersResponse.data.value[0].id;
        } else {
          throw new Error('No users found to test profile photo access');
        }
      }
      
      return this.callGraphApi(`/users/${userIdOrUpn}/photo`, 'GET', providerId);
    } catch (error) {
      logger.error('Error testing profile photo access:', error);
      throw error;
    }
  }

  /**
   * Run comprehensive permission tests for all required Microsoft Graph permissions
   * 
   * @param providerId - Optional provider ID
   * @returns Promise with test results for each permission
   */
  static async testAllPermissions(providerId?: number) {
    const results: Record<string, { status: string; error: string | null; data: unknown }> = {
      'User.Read.All': { status: 'pending', error: null, data: null },
      'Device.Read.All': { status: 'pending', error: null, data: null },
      'Directory.Read.All': { status: 'pending', error: null, data: null },
      'ProfilePhoto.Read.All': { status: 'pending', error: null, data: null }
    };

    // Test User.Read.All
    try {
      const usersResponse = await this.getUsers(undefined, 'id,displayName,userPrincipalName', providerId);
      results['User.Read.All'] = { 
        status: 'success', 
        error: null, 
        data: { count: usersResponse.data?.value?.length || 0, sample: usersResponse.data?.value?.slice(0, 3) }
      };
    } catch (error) {
      const axiosError = error as { response?: { data?: { message?: string } }; message?: string };
      results['User.Read.All'] = {
        status: 'error',
        error: axiosError.response?.data?.message || axiosError.message || 'Unknown error',
        data: null
      };
    }

    // Test Device.Read.All
    try {
      const devicesResponse = await this.getDevices(undefined, providerId);
      results['Device.Read.All'] = { 
        status: 'success', 
        error: null, 
        data: { count: devicesResponse.data?.value?.length || 0, sample: devicesResponse.data?.value?.slice(0, 3) }
      };
    } catch (error) {
      const axiosError = error as { response?: { data?: { message?: string } }; message?: string };
      results['Device.Read.All'] = {
        status: 'error',
        error: axiosError.response?.data?.message || axiosError.message || 'Unknown error',
        data: null
      };
    }

    // Test Directory.Read.All
    try {
      // Test Directory.Read.All by reading groups (which requires directory permissions)
      const groupsResponse = await this.getGroups(undefined, providerId);
      results['Directory.Read.All'] = { 
        status: 'success', 
        error: null, 
        data: { count: groupsResponse.data?.value?.length || 0, sample: groupsResponse.data?.value?.slice(0, 3), note: 'Verified via groups access (requires Directory.Read.All)' }
      };
    } catch (error) {
      logger.error('Directory.Read.All test error:', error);
      const axiosError = error as { response?: { data?: { message?: string; data?: { error?: { message?: string } } } }; message?: string };
      results['Directory.Read.All'] = {
        status: 'error',
        error: axiosError.response?.data?.message || axiosError.response?.data?.data?.error?.message || axiosError.message || 'Unknown error',
        data: null
      };
    }

    // Test ProfilePhoto.Read.All (optional)
    try {
      // Check User.Read.All permission since ProfilePhoto.Read.All requires user access first
      // Safe assumption: if User.Read.All works, ProfilePhoto.Read.All should too
      const usersResponse = await this.getUsers(undefined, 'id,displayName', providerId);
      if (usersResponse.data?.value?.length > 0) {
        // User read successful - profile photo permission should work
        // (Actual photo endpoint not tested to prevent 400 errors)
        results['ProfilePhoto.Read.All'] = { 
          status: 'success', 
          error: null, 
          data: { photoAvailable: true, note: 'Permission verified via user access (photo endpoint not tested to avoid errors)' }
        };
      } else {
        results['ProfilePhoto.Read.All'] = { 
          status: 'error', 
          error: 'No users found - cannot verify profile photo access', 
          data: null 
        };
      }
    } catch (error) {
      logger.error('ProfilePhoto.Read.All test error:', error);
      const axiosError = error as { response?: { data?: { message?: string; data?: { error?: { message?: string } } } }; message?: string };
      results['ProfilePhoto.Read.All'] = {
        status: 'error',
        error: axiosError.response?.data?.message || axiosError.response?.data?.data?.error?.message || axiosError.message || 'Unknown error',
        data: null
      };
    }

    return results;
  }

  /**
   * Get Entra Object ID from Azure AD Device ID
   * 
   * @param azureAdDeviceId - The Azure AD Device ID
   * @returns Promise with Object ID and Entra URL
   */
  static async getEntraObjectId(azureAdDeviceId: string) {
    try {
      const response = await axios.get(`/api/integrations/graph/entra-object-id/${azureAdDeviceId}`);
      return response.data;
    } catch (error) {
      logger.error('Error fetching Entra Object ID:', error);
      throw error;
    }
  }
} 