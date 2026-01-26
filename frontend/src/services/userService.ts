import apiClient from './apiConfig';
import { API_URL } from './apiConfig';
import { logger } from '@/utils/logger';
import { RequestManager } from '@/utils/requestManager';
import type { PaginationParams, PaginatedResponse } from '@/types/pagination';
import type { User } from '@/types/user';

// Re-export for backwards compatibility
export type { User };

// Extended pagination params for users
export interface UserPaginationParams extends PaginationParams {
  role?: string;
}

// Re-export for backwards compatibility
export type { PaginatedResponse } from '@/types/pagination';

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

// Request cancellation manager instance
const requestManager = new RequestManager();

// Service for user-related API calls
const userService = {
  // Get all users
  async getAllUsers(): Promise<User[]> {
    try {
      const response = await apiClient.get('/users');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to fetch all users', { error });
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
      logger.error('Failed to fetch users batch', { error, uuidCount: uuids.length });
      return [];
    }
  },

  // Get paginated users with cancellation support
  async getPaginatedUsers(params: UserPaginationParams, requestKey: string = 'paginated-users'): Promise<PaginatedResponse<User>> {
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
    } catch (error) {
      // Don't throw if request was cancelled
      const errorWithName = error as { name?: string };
      if (errorWithName.name === 'AbortError' || errorWithName.name === 'CanceledError') {
        logger.debug('Request cancelled', { requestKey });
        throw new Error('REQUEST_CANCELLED');
      }
      logger.error('Failed to fetch paginated users', { error, params });
      throw error;
    }
  },

  // Get a user by UUID
  async getUserByUuid(uuid: string): Promise<User | null> {
    try {
      const response = await apiClient.get(`/users/${uuid}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to fetch user by UUID', { error, uuid });
      return null;
    }
  },

  // Get user email addresses
  async getUserEmails(uuid: string): Promise<UserEmail[]> {
    try {
      const response = await apiClient.get(`/users/${uuid}/emails`);
      return response.data.emails || [];
    } catch (error) {
      logger.error('Failed to fetch user emails', { error, uuid });
      return [];
    }
  },

  // Add a new email address
  async addUserEmail(uuid: string, email: string): Promise<UserEmail | null> {
    try {
      const response = await apiClient.post(`/users/${uuid}/emails`, { email });
      return response.data.email || null;
    } catch (error) {
      logger.error('Failed to add user email', { error, uuid, email });
      throw error;
    }
  },

  // Update email (set as primary or verified)
  async updateUserEmail(uuid: string, emailId: number, updates: { is_primary?: boolean; is_verified?: boolean }): Promise<UserEmail | null> {
    try {
      const response = await apiClient.put(`/users/${uuid}/emails/${emailId}`, updates);
      return response.data.email || null;
    } catch (error) {
      logger.error('Failed to update user email', { error, uuid, emailId, updates });
      throw error;
    }
  },

  // Delete an email address
  async deleteUserEmail(uuid: string, emailId: number): Promise<void> {
    try {
      await apiClient.delete(`/users/${uuid}/emails/${emailId}`);
    } catch (error) {
      logger.error('Failed to delete user email', { error, uuid, emailId });
      throw error;
    }
  },

  // Create a new user
  async createUser(user: {
    name: string;
    email: string;
    role: string;
    pronouns?: string;
    password?: string;
    send_invitation?: boolean;
  }): Promise<User> {
    // Send user data - backend generates UUID and sets all defaults
    // If send_invitation is true, backend will send email invite
    // If password is provided, user can log in immediately
    const payload: Record<string, unknown> = {
      name: user.name.trim(),
      email: user.email.trim().toLowerCase(),
      role: user.role,
      pronouns: user.pronouns || null,
    };

    // Include password if provided (for when SMTP is not configured)
    if (user.password) {
      payload.password = user.password;
    }

    // Include send_invitation flag
    if (user.send_invitation !== undefined) {
      payload.send_invitation = user.send_invitation;
    }

    try {
      const response = await apiClient.post(`/users`, payload);
      return response.data;
    } catch (error: unknown) {
      logger.error('Failed to create user', { error, email: user.email });
      // Extract error message from backend response
      const axiosError = error as { response?: { data?: { message?: string } } };
      const message = axiosError.response?.data?.message || 'Failed to create user';
      throw new Error(message);
    }
  },

  // Update a user
  async updateUser(uuid: string, userData: Partial<User>): Promise<User | null> {
    try {
      const response = await apiClient.put(`/users/${uuid}`, userData);
      return response.data;
    } catch (error) {
      logger.error('Failed to update user', { error, uuid });
      return null;
    }
  },

  // Delete a user
  async deleteUser(uuid: string): Promise<boolean> {
    try {
      await apiClient.delete(`/users/${uuid}`);
      return true;
    } catch (error) {
      logger.error('Failed to delete user', { error, uuid });
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
          logger.error('Failed to parse user data from localStorage', { error: e });
        }
      }

      // With UUID available, use the UUID-based endpoint
      const endpoint = userUuid ?
        `/users/${userUuid}/auth-identities` :
        `/users/auth-identities`;

      logger.debug('Fetching auth identities', { endpoint });
      const response = await apiClient.get(endpoint);

      // Validate response data
      if (!response.data || !Array.isArray(response.data)) {
        logger.error('Invalid auth identities response format', { data: response.data });
        return [];
      }

      logger.debug('Fetched auth identities from API', { count: response.data.length });
      
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
              logger.warn('Failed to parse identity_data JSON', { error: e });
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

      logger.debug('Processed auth identities', { count: validIdentities.length });
      return validIdentities;
    } catch (error) {
      logger.error('Failed to fetch user auth identities', { error });
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
          logger.error('Failed to parse user data from localStorage', { error: e });
        }
      }

      // With UUID available, use the UUID-based endpoint
      const endpoint = userUuid ?
        `/users/${userUuid}/auth-identities/${identityId}` :
        `/users/auth-identities/${identityId}`;

      logger.debug('Deleting auth identity', { endpoint, identityId });
      await apiClient.delete(endpoint);
      return true;
    } catch (error) {
      logger.error('Failed to delete auth identity', { error, identityId });
      return false;
    }
  },

  // Upload image and return the URL path
  async uploadImage(
    file: File,
    type: 'avatar' | 'banner',
    targetUserUuid?: string,
    onProgress?: (progress: number) => void
  ): Promise<string | null> {
    try {
      // Use the provided UUID, or fetch the current user's UUID if not provided
      let userUuid = targetUserUuid || '';

      if (!userUuid) {
        try {
          const token = localStorage.getItem('token');
          if (token) {
            // Make a request to get current user to ensure the correct UUID is available
            const userResponse = await apiClient.get('/auth/me');
            if (userResponse.data && userResponse.data.uuid) {
              userUuid = userResponse.data.uuid;
              logger.debug('Retrieved user UUID from /auth/me endpoint', { userUuid });

              // Update localStorage with fresh user data
              localStorage.setItem('user', JSON.stringify(userResponse.data));
            }
          }
        } catch (e) {
          logger.error('Failed to fetch current user data', { error: e });
        }
      }

      if (!userUuid) {
        logger.error('No user UUID found for image upload');
        return null;
      }

      logger.debug('Uploading image for user', { userUuid, type });

      // Create form data
      const formData = new FormData();
      formData.append('file', file);

      // Upload the file using the new endpoint
      const response = await apiClient.post(`/users/${userUuid}/image?type_=${type}`, formData, {
        headers: {
          'Content-Type': 'multipart/form-data'
        },
        onUploadProgress: onProgress ? (progressEvent) => {
          const progress = progressEvent.total
            ? Math.round((progressEvent.loaded * 100) / progressEvent.total)
            : 0;
          onProgress(progress);
        } : undefined
      });

      logger.debug('Image upload response received', { type });

      // Return the URL
      if (response.data && response.data.url) {
        logger.info('Image upload successful', { type, url: response.data.url });
        return response.data.url;
      } else if (response.data && response.data.user && type === 'avatar' && response.data.user.avatar_url) {
        logger.info('Avatar upload successful', { url: response.data.user.avatar_url });
        return response.data.user.avatar_url;
      } else if (response.data && response.data.user && type === 'banner' && response.data.user.banner_url) {
        logger.info('Banner upload successful', { url: response.data.user.banner_url });
        return response.data.user.banner_url;
      }

      logger.warn('Upload response did not contain a URL', { type, data: response.data });
      return null;
    } catch (error) {
      logger.error('Failed to upload image', { error, type });
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
      logger.error('Failed to generate MFA setup', { error });
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
      logger.error('Failed to verify MFA setup', { error });
      return null;
    }
  },

  // Enable MFA for user
  async enableMfa(token: string): Promise<boolean> {
    try {
      await apiClient.post('/auth/mfa/enable', { token });
      return true;
    } catch (error) {
      logger.error('Failed to enable MFA', { error });
      return false;
    }
  },

  // Disable MFA for user
  async disableMfa(password: string): Promise<boolean> {
    try {
      await apiClient.post('/auth/mfa/disable', { password });
      return true;
    } catch (error) {
      logger.error('Failed to disable MFA', { error });
      return false;
    }
  },

  // Regenerate backup codes
  async regenerateBackupCodes(password: string): Promise<string[] | null> {
    try {
      const response = await apiClient.post('/auth/mfa/regenerate-backup-codes', { password });
      return response.data.backup_codes;
    } catch (error) {
      logger.error('Failed to regenerate backup codes', { error });
      return null;
    }
  },

  // Get user MFA status
  async getMfaStatus(): Promise<{ enabled: boolean; has_backup_codes: boolean } | null> {
    try {
      const response = await apiClient.get('/auth/mfa/status');
      return response.data;
    } catch (error) {
      logger.error('Failed to get MFA status', { error });
      return null;
    }
  },

  // Login with MFA token
  async loginWithMfa(email: string, password: string, mfaToken: string): Promise<{
    success: boolean;
    mfa_required?: boolean;
    token?: string;
    user?: User;
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
    } catch (error) {
      logger.error('Failed to login with MFA', { error, email });
      // Return error response data if available for better error handling
      const axiosError = error as { response?: { data?: { message?: string } } };
      if (axiosError.response?.data) {
        return {
          success: false,
          message: axiosError.response.data.message || 'MFA login failed'
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
    } catch (error) {
      logger.error('Failed to cleanup stale images', { error });
      const axiosError = error as { response?: { data?: { message?: string } } };
      return {
        success: false,
        message: axiosError.response?.data?.message || 'Failed to cleanup stale images'
      };
    }
  },

  // Get email configuration status (admin only)
  async getEmailConfigStatus(): Promise<{ is_configured: boolean; enabled: boolean }> {
    try {
      const response = await apiClient.get('/admin/email/config');
      return {
        is_configured: response.data.is_configured || false,
        enabled: response.data.enabled || false
      };
    } catch (error) {
      logger.error('Failed to get email config status', { error });
      return { is_configured: false, enabled: false };
    }
  },

  // Resend invitation email to a user who hasn't completed account setup
  async resendInvitation(uuid: string): Promise<{ success: boolean; message: string; email?: string }> {
    try {
      const response = await apiClient.post(`/users/${uuid}/resend-invitation`);
      return {
        success: true,
        message: response.data.message || 'Invitation email sent successfully',
        email: response.data.email
      };
    } catch (error) {
      logger.error('Failed to resend invitation', { error, uuid });
      const axiosError = error as { response?: { data?: { message?: string } } };
      return {
        success: false,
        message: axiosError.response?.data?.message || 'Failed to send invitation email'
      };
    }
  },

  // Bulk operations on users (admin only)
  async bulkAction(request: { action: 'delete' | 'set-role'; ids: string[]; value?: string }): Promise<{ affected: number }> {
    const response = await apiClient.post('/users/bulk', request);
    return response.data;
  }
};

export default userService; 