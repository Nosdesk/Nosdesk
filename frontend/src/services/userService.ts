import axios from 'axios';
import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import { RequestManager } from '@/utils/requestManager';
import type { PaginationParams, PaginatedResponse } from '@/types/pagination';
import type { User, UserRole, UserSecurityInfo } from '@/types/user';
import type { Asset } from '@/types/asset';
import type { Group } from '@/types/group';
import { extractErrorMessage } from '@/utils/errors';

// Re-export for backwards compatibility
export type { User };

// Extended pagination params for users
export interface UserPaginationParams extends PaginationParams {
  role?: string;
  /** Filter on soft-delete state. `"active"` (default) hides
   * soft-deleted rows; `"deleted"` shows only them; `"all"`
   * returns both. The admin "Deleted users" view flips this to
   * `"deleted"`. */
  deleted?: 'active' | 'deleted' | 'all';
}

// Re-export for backwards compatibility
export type { PaginatedResponse } from '@/types/pagination';

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

/** Sub-resource keys the `/users/{uuid}/profile` endpoint
 *  understands. Keep in sync with `ProfileGroup` in
 *  `backend/src/repository/user_profile.rs`. */
export type ProfileBundleGroup = 'devices' | 'groups' | 'emails' | 'counts'

export interface UserProfileCounts {
  assignedTickets: number
  requestedTickets: number
}

/** Sparse bundle: `user` is always present, every other field is
 *  only included when the corresponding key was in `?include=`. */
export interface UserProfileBundle {
  user: User
  devices?: Asset[]
  groups?: Group[]
  emails?: UserEmail[]
  counts?: UserProfileCounts
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

  // Get paginated users with cancellation support.
  //
  // Two cancellation modes:
  //   1. Caller-supplied `options.signal`: standard spec
  //      AbortController flow. Cancellation surfaces as a native
  //      `AbortError` and the internal `requestManager` is bypassed.
  //      Used by composables that own their own controller (e.g.
  //      `useUserMentionSearch`).
  //   2. Legacy `requestKey` mode (default): `requestManager`
  //      cancels the prior in-flight request that shares the same
  //      key, and cancellations are re-thrown as the stringly-typed
  //      `Error('REQUEST_CANCELLED')` sentinel. Preserved for older
  //      call sites that haven't migrated.
  async getPaginatedUsers(
    params: UserPaginationParams,
    requestKey: string = 'paginated-users',
    options: { signal?: AbortSignal } = {},
  ): Promise<PaginatedResponse<User>> {
    if (options.signal) {
      try {
        const response = await apiClient.get('/users/paginated', {
          params,
          signal: options.signal,
        });
        return response.data;
      } catch (error) {
        if (axios.isCancel(error)) {
          // Re-throw as the spec-compliant DOMException-style
          // AbortError so callers can branch on `err.name`.
          const aborted = new Error('Aborted');
          aborted.name = 'AbortError';
          throw aborted;
        }
        logger.error('Failed to fetch paginated users', { error, params });
        throw error;
      }
    }

    try {
      const controller = requestManager.createRequest(requestKey);
      const response = await apiClient.get('/users/paginated', {
        params,
        signal: controller.signal,
      });
      requestManager.cancelRequest(requestKey);
      return response.data;
    } catch (error) {
      if (axios.isCancel(error)) {
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

  // Bundled profile read for the user profile page. Pass the
  // sub-resource keys you actually render, the backend computes and
  // serialises only those (sparse fieldsets). Throws on network /
  // 4xx / 5xx so the caller can surface the failure, the legacy
  // single-fetch helpers above swallow errors for backwards-compat.
  async getUserProfileBundle(
    uuid: string,
    include: ProfileBundleGroup[],
  ): Promise<UserProfileBundle> {
    const response = await apiClient.get<UserProfileBundle>(
      `/users/${uuid}/profile`,
      { params: { include: include.join(',') } },
    );
    return response.data;
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
      const message = extractErrorMessage(error, 'Failed to create user');
      throw new Error(message);
    }
  },

  // Update a user
  async updateUser(
    uuid: string,
    // `role` is a write-only convenience: the backend update endpoint
    // accepts a legacy role tier string and re-derives platform_role +
    // workspace_role from it. It isn't a field on `User` (the response
    // carries the split), so allow it explicitly here.
    userData: Partial<User> & { role?: UserRole },
  ): Promise<User | null> {
    try {
      const response = await apiClient.put(`/users/${uuid}`, userData);
      return response.data;
    } catch (error) {
      logger.error('Failed to update user', { error, uuid });
      return null;
    }
  },

  /**
   * Soft-delete a user. Backend stamps `deleted_at` and emits
   * `user-soft-deleted`; the retention worker hard-deletes after
   * the configured grace window. Returns the new deleted_at +
   * purge_at so the caller can show "delete in N days".
   */
  async deleteUser(uuid: string): Promise<
    { deleted_at: string; purge_at: string } | null
  > {
    try {
      const { data } = await apiClient.delete<{
        uuid: string;
        deleted_at: string;
        purge_at: string;
      }>(`/users/${uuid}`);
      return { deleted_at: data.deleted_at, purge_at: data.purge_at };
    } catch (error) {
      logger.error('Failed to delete user', { error, uuid });
      return null;
    }
  },

  /** Restore a soft-deleted user. Idempotent on the FE side; the
   *  backend 409s if the user wasn't soft-deleted. */
  async restoreUser(uuid: string): Promise<boolean> {
    try {
      await apiClient.post(`/users/${uuid}/restore`);
      return true;
    } catch (error) {
      logger.error('Failed to restore user', { error, uuid });
      return false;
    }
  },

  /** Permanently delete a soft-deleted user (GDPR erasure). The
   *  backend refuses unless `deleted_at` is already set; route the
   *  user through `deleteUser` first if they're still active. */
  async purgeUserNow(uuid: string): Promise<boolean> {
    try {
      await apiClient.delete(`/users/${uuid}/purge`);
      return true;
    } catch (error) {
      logger.error('Failed to permanently delete user', { error, uuid });
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
      return {
        success: false,
        message: extractErrorMessage(error, 'Failed to cleanup stale images')
      };
    }
  },

  // Regenerate avatar thumbnails missing on disk or unset in the DB
  // (admin only). Idempotent; mirrors the restore-time and scheduled
  // backfill so all three paths behave the same.
  async regenerateThumbnails(): Promise<{
    success: boolean;
    message?: string;
    stats?: {
      checked: number;
      regenerated: number;
      failed: number;
    };
  }> {
    try {
      const response = await apiClient.post('/users/regenerate-thumbnails');
      return response.data;
    } catch (error) {
      logger.error('Failed to regenerate thumbnails', { error });
      return {
        success: false,
        message: extractErrorMessage(error, 'Failed to regenerate thumbnails')
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
      return {
        success: false,
        message: extractErrorMessage(error, 'Failed to send invitation email')
      };
    }
  },

  // Bulk operations on users (admin only)
  async bulkAction(request: { action: 'delete' | 'set-role'; ids: string[]; value?: string }): Promise<{ affected: number }> {
    const response = await apiClient.post('/users/bulk', request);
    return response.data;
  },

  // Get security info for a user (admin or self)
  async getUserSecurityInfo(uuid: string): Promise<UserSecurityInfo> {
    const response = await apiClient.get(`/users/${uuid}/security-info`);
    return response.data;
  },

  // Admin: reset a user's password
  async adminResetUserPassword(uuid: string, newPassword: string): Promise<void> {
    await apiClient.post(`/users/${uuid}/reset-password`, { new_password: newPassword });
  },

  // Admin: disable MFA for a user
  async adminDisableUserMfa(uuid: string): Promise<void> {
    await apiClient.post(`/users/${uuid}/disable-mfa`);
  },

  // Admin: delete a passkey for a user
  async adminDeleteUserPasskey(uuid: string, credentialId: string): Promise<void> {
    await apiClient.delete(`/users/${uuid}/passkeys/${credentialId}`);
  },

  // Admin: remove an auth identity for a user
  async adminDeleteUserAuthIdentity(uuid: string, identityId: number): Promise<void> {
    await apiClient.delete(`/users/${uuid}/auth-identities/${identityId}`);
  }
};

export default userService; 