import apiClient from './apiConfig';
import { API_URL } from './apiConfig';

// User interface matching the backend model
export interface User {
  id: number;
  uuid: string;
  name: string;
  email: string;
  role: string;
  pronouns?: string | null;
  avatar_url?: string | null;
  banner_url?: string | null;
  avatar_thumb?: string | null;
  created_at: string;
  updated_at: string;
}

// Pagination interface
export interface PaginationParams {
  page: number;
  pageSize: number;
  sortField?: string;
  sortDirection?: 'asc' | 'desc';
  search?: string;
  role?: string;
}

// Paginated response interface
export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

// Auth Identity interface
export interface AuthIdentity {
  id: number;
  provider_type: string;
  provider_name: string;
  email: string | null;
  created_at: string;
}

// User Email interface matching the backend model
export interface UserEmail {
  id: number;
  user_id: number;
  email: string;
  email_type: string;
  is_primary: boolean;
  verified: boolean;
  source?: string | null;
  created_at: string;
  updated_at: string;
}

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

// Service for user-related API calls
const userService = {
  // Get all users
  async getAllUsers(): Promise<User[]> {
    try {
      const response = await apiClient.get('/users');
      return response.data || [];
    } catch (error) {
      console.error('Error fetching all users:', error);
      return [];
    }
  },

  // Get multiple users by UUIDs in a single request
  async getUsersBatch(uuids: string[]): Promise<User[]> {
    try {
      // Remove duplicates and empty values
      const uniqueUuids = [...new Set(uuids.filter(uuid => uuid && uuid.trim()))];
      
      if (uniqueUuids.length === 0) {
        return [];
      }

      const response = await apiClient.post('/users/batch', {
        uuids: uniqueUuids
      });
      return response.data || [];
    } catch (error) {
      console.error('Error fetching users batch:', error);
      return [];
    }
  },

  // Get paginated users with cancellation support
  async getPaginatedUsers(params: PaginationParams, requestKey: string = 'paginated-users'): Promise<PaginatedResponse<User>> {
    try {
      // Create cancellable request
      const controller = requestManager.createRequest(requestKey);
      
      const response = await apiClient.get(`/users/paginated`, { 
        params,
        signal: controller.signal 
      });
      
      // Remove from active requests on success
      requestManager.cancelRequest(requestKey);
      
      return response.data;
    } catch (error: any) {
      // Don't throw if request was cancelled
      if (error.name === 'AbortError' || error.name === 'CanceledError') {
        console.log('Request cancelled:', requestKey);
        throw new Error('REQUEST_CANCELLED');
      }
      console.error('Error fetching paginated users:', error);
      throw error;
    }
  },

  // Get a user by UUID
  async getUserByUuid(uuid: string): Promise<User | null> {
    try {
      const response = await apiClient.get(`/users/${uuid}`);
      return response.data;
    } catch (error) {
      console.error(`Error fetching user with UUID ${uuid}:`, error);
      return null;
    }
  },

  // Get user email addresses
  async getUserEmails(uuid: string): Promise<UserEmail[]> {
    try {
      const response = await apiClient.get(`/users/${uuid}/emails`);
      return response.data.emails || [];
    } catch (error: any) {
      console.error(`Error fetching emails for user with UUID ${uuid}:`, error);
      return [];
    }
  },

  // Add a new email address
  async addUserEmail(uuid: string, email: string): Promise<UserEmail | null> {
    try {
      const response = await apiClient.post(`/users/${uuid}/emails`, { email });
      return response.data.email || null;
    } catch (error: any) {
      console.error(`Error adding email for user ${uuid}:`, error);
      throw error;
    }
  },

  // Update email (set as primary or verified)
  async updateUserEmail(uuid: string, emailId: number, updates: { is_primary?: boolean; is_verified?: boolean }): Promise<UserEmail | null> {
    try {
      const response = await apiClient.put(`/users/${uuid}/emails/${emailId}`, updates);
      return response.data.email || null;
    } catch (error: any) {
      console.error(`Error updating email ${emailId}:`, error);
      throw error;
    }
  },

  // Delete an email address
  async deleteUserEmail(uuid: string, emailId: number): Promise<void> {
    try {
      await apiClient.delete(`/users/${uuid}/emails/${emailId}`);
    } catch (error: any) {
      console.error(`Error deleting email ${emailId}:`, error);
      throw error;
    }
  },

  // Create a new user
  async createUser(user: { name: string; email: string; role: string; pronouns?: string }): Promise<User | null> {
    try {
      // Generate a UUID for the new user
      const userUuid = crypto.randomUUID();
      
      // Create a NewUser object that matches the backend expectations
      const newUser = {
        uuid: userUuid,
        name: user.name.trim(),
        email: user.email.trim().toLowerCase(),
        role: user.role, // The backend will convert this to the enum
        password_hash: [], // Empty array - backend will set default password
        pronouns: user.pronouns || null,
        avatar_url: null,
        banner_url: null,
        avatar_thumb: null,
        microsoft_uuid: null,
        mfa_secret: null,
        mfa_enabled: false,
        mfa_backup_codes: null,
        passkey_credentials: null
      };
      
      const response = await apiClient.post(`/users`, newUser);
      return response.data;
    } catch (error) {
      console.error('Error creating user:', error);
      return null;
    }
  },

  // Update a user
  async updateUser(uuid: string, userData: Partial<User>): Promise<User | null> {
    try {
      const response = await apiClient.put(`/users/${uuid}`, userData);
      return response.data;
    } catch (error) {
      console.error(`Error updating user with UUID ${uuid}:`, error);
      return null;
    }
  },

  // Delete a user
  async deleteUser(uuid: string): Promise<boolean> {
    try {
      await apiClient.delete(`/users/${uuid}`);
      return true;
    } catch (error) {
      console.error(`Error deleting user with UUID ${uuid}:`, error);
      return false;
    }
  },

  // Get user authentication identities
  async getUserAuthIdentities(): Promise<AuthIdentity[]> {
    try {
      // Get the current user from localStorage to get UUID
      const userJson = localStorage.getItem('user');
      let userUuid = '';
      
      if (userJson) {
        try {
          const userData = JSON.parse(userJson);
          userUuid = userData.uuid;
        } catch (e) {
          console.error('Error parsing user data from localStorage:', e);
        }
      }
      
      // If we have the UUID, use the new UUID-based endpoint
      const endpoint = userUuid ? 
        `/users/${userUuid}/auth-identities` : 
        `/users/auth-identities`;
        
      console.log(`Using auth identities endpoint: ${endpoint}`);
      const response = await apiClient.get(endpoint);
      
      // Validate response data
      if (!response.data || !Array.isArray(response.data)) {
        console.error('Invalid auth identities response format:', response.data);
        return [];
      }
      
      console.log('Raw auth identities from API:', response.data);
      
      // Process and validate each identity
      const validIdentities = response.data
        .filter(item => 
          item && 
          typeof item === 'object' && 
          typeof item.id === 'number'
        )
        .map(item => {
          // Get the provider details - the backend might send different field names
          let providerType = 'unknown';
          let providerName = 'Unknown Provider';
          let email = item.email || null;
          
          // Try to extract provider type from the data
          if (item.provider_type) {
            providerType = item.provider_type;
          } else if (item.auth_provider && item.auth_provider.provider_type) {
            // If the backend sends a nested auth_provider object
            providerType = item.auth_provider.provider_type;
          } else if (item.auth_provider_id === 2) {
            // Hardcoded mapping for known provider IDs (from your database)
            providerType = 'microsoft';
          } else if (item.auth_provider_id === 1) {
            providerType = 'local';
          }
          
          // Try to get provider name
          if (item.provider_name) {
            providerName = item.provider_name;
          } else if (item.auth_provider && item.auth_provider.name) {
            providerName = item.auth_provider.name;
          } else {
            // Fallback names based on type
            switch(providerType) {
              case 'microsoft':
                providerName = 'Microsoft Account';
                break;
              case 'local':
                providerName = 'Local Account';
                break;
              default:
                providerName = providerType.charAt(0).toUpperCase() + providerType.slice(1);
            }
          }
          
          // Extract additional information from identity_data if available
          if (item.identity_data) {
            try {
              // Parse identity_data if it's a string
              const identityData = typeof item.identity_data === 'string' 
                ? JSON.parse(item.identity_data)
                : item.identity_data;
                
              // For Microsoft accounts
              if (providerType === 'microsoft' && identityData) {
                // Use more detailed information if available
                if (identityData.displayName) {
                  providerName = `Microsoft (${identityData.displayName})`;
                }
                
                // Get email from identity data if not already set
                if (!email && identityData.mail) {
                  email = identityData.mail;
                } else if (!email && identityData.userPrincipalName) {
                  email = identityData.userPrincipalName;
                }
              }
            } catch (e) {
              console.warn('Failed to parse identity_data JSON:', e);
            }
          }
          
          return {
            id: item.id,
            provider_type: providerType,
            provider_name: providerName,
            email: email,
            created_at: item.created_at || new Date().toISOString()
          };
        });
      
      console.log('Processed auth identities:', validIdentities);
      return validIdentities;
    } catch (error) {
      console.error('Error fetching user auth identities:', error);
      return [];
    }
  },

  // Delete a user authentication identity
  async deleteUserAuthIdentity(identityId: number): Promise<boolean> {
    try {
      // Get the current user from localStorage to get UUID
      const userJson = localStorage.getItem('user');
      let userUuid = '';
      
      if (userJson) {
        try {
          const userData = JSON.parse(userJson);
          userUuid = userData.uuid;
        } catch (e) {
          console.error('Error parsing user data from localStorage:', e);
        }
      }
      
      // If we have the UUID, use the new UUID-based endpoint
      const endpoint = userUuid ? 
        `/users/${userUuid}/auth-identities/${identityId}` : 
        `/users/auth-identities/${identityId}`;
        
      console.log(`Using delete auth identity endpoint: ${endpoint}`);
      await apiClient.delete(endpoint);
      return true;
    } catch (error) {
      console.error(`Error deleting auth identity with ID ${identityId}:`, error);
      return false;
    }
  },

  // Upload image and return the URL path
  async uploadImage(file: File, type: 'avatar' | 'banner', targetUserUuid?: string): Promise<string | null> {
    try {
      // Use the provided UUID, or fetch the current user's UUID if not provided
      let userUuid = targetUserUuid || '';

      if (!userUuid) {
        try {
          const token = localStorage.getItem('token');
          if (token) {
            // Make a request to get current user to ensure we have the correct UUID
            const userResponse = await apiClient.get('/auth/me');
            if (userResponse.data && userResponse.data.uuid) {
              userUuid = userResponse.data.uuid;
              console.log('Retrieved user UUID from /auth/me endpoint:', userUuid);

              // Update localStorage with fresh user data
              localStorage.setItem('user', JSON.stringify(userResponse.data));
            }
          }
        } catch (e) {
          console.error('Error fetching current user data:', e);
        }
      }

      if (!userUuid) {
        console.error('No user UUID found for image upload');
        return null;
      }
      
      console.log(`Using user UUID for ${type} upload:`, userUuid);
      
      // Create form data
      const formData = new FormData();
      formData.append('file', file);
      
      // Upload the file using the new endpoint
      const response = await apiClient.post(`/users/${userUuid}/image?type_=${type}`, formData, {
        headers: {
          'Content-Type': 'multipart/form-data'
        }
      });
      
      console.log(`${type} upload response:`, response.data);
      
      // Return the URL
      if (response.data && response.data.url) {
        console.log(`Upload successful, received URL: ${response.data.url}`);
        return response.data.url;
      } else if (response.data && response.data.user && type === 'avatar' && response.data.user.avatar_url) {
        console.log(`Avatar upload successful, using avatar_url from response: ${response.data.user.avatar_url}`);
        return response.data.user.avatar_url;
      } else if (response.data && response.data.user && type === 'banner' && response.data.user.banner_url) {
        console.log(`Banner upload successful, using banner_url from response: ${response.data.user.banner_url}`);
        return response.data.user.banner_url;
      }
      
      console.warn('Upload response did not contain a URL:', response.data);
      return null;
    } catch (error) {
      console.error(`Error uploading ${type} image:`, error);
      return null;
    }
  },

  // MFA (Multi-Factor Authentication) related functions
  
  // Generate MFA secret and QR code for setup
  async generateMfaSetup(): Promise<{ secret: string; qr_code: string; backup_codes: string[] } | null> {
    try {
      const response = await apiClient.post('/auth/mfa/setup');
      return response.data;
    } catch (error) {
      console.error('Error generating MFA setup:', error);
      return null;
    }
  },

  // Verify MFA token during setup
  async verifyMfaSetup(token: string, secret: string): Promise<{ success: boolean; backup_codes: string[] } | null> {
    try {
      const response = await apiClient.post('/auth/mfa/verify-setup', {
        token,
        secret
      });
      return response.data;
    } catch (error) {
      console.error('Error verifying MFA setup:', error);
      return null;
    }
  },

  // Enable MFA for user
  async enableMfa(token: string): Promise<boolean> {
    try {
      await apiClient.post('/auth/mfa/enable', { token });
      return true;
    } catch (error) {
      console.error('Error enabling MFA:', error);
      return false;
    }
  },

  // Disable MFA for user
  async disableMfa(password: string): Promise<boolean> {
    try {
      await apiClient.post('/auth/mfa/disable', { password });
      return true;
    } catch (error) {
      console.error('Error disabling MFA:', error);
      return false;
    }
  },

  // Regenerate backup codes
  async regenerateBackupCodes(password: string): Promise<string[] | null> {
    try {
      const response = await apiClient.post('/auth/mfa/regenerate-backup-codes', { password });
      return response.data.backup_codes;
    } catch (error) {
      console.error('Error regenerating backup codes:', error);
      return null;
    }
  },

  // Get user MFA status
  async getMfaStatus(): Promise<{ enabled: boolean; has_backup_codes: boolean } | null> {
    try {
      const response = await apiClient.get('/auth/mfa/status');
      return response.data;
    } catch (error) {
      console.error('Error getting MFA status:', error);
      return null;
    }
  },

  // Login with MFA token
  async loginWithMfa(email: string, password: string, mfaToken: string): Promise<{
    success: boolean;
    mfa_required?: boolean;
    token?: string;
    user?: any;
    message?: string;
    mfa_backup_code_used?: boolean;
    requires_backup_code_regeneration?: boolean;
  } | null> {
    try {
      const response = await apiClient.post('/auth/mfa-login', {
        email,
        password,
        mfa_token: mfaToken
      });
      return response.data;
    } catch (error: any) {
      console.error('Error logging in with MFA:', error);
      // Return error response data if available for better error handling
      if (error.response?.data) {
        return {
          success: false,
          message: error.response.data.message || 'MFA login failed'
        };
      }
      return null;
    }
  },

  // Cancel all active requests
  cancelAllRequests(): void {
    requestManager.cancelAllRequests();
  },

  // Cleanup stale images (avatars, banners, thumbnails)
  async cleanupStaleImages(): Promise<{
    success: boolean;
    message: string;
    stats?: {
      avatars_removed: number;
      banners_removed: number;
      thumbnails_removed?: number;
      total_files_checked: number;
      errors: string[];
    };
  }> {
    try {
      const response = await apiClient.post('/users/cleanup-images');
      return response.data;
    } catch (error: any) {
      console.error('Error cleaning up stale images:', error);
      return {
        success: false,
        message: error.response?.data?.message || 'Failed to cleanup stale images'
      };
    }
  }
};

export default userService; 