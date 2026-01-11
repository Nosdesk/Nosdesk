import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type { ApiToken, ApiTokenCreated, CreateApiTokenRequest } from '@/types/apiToken';

/**
 * API Token Service
 * Manages API tokens for programmatic access
 */
const apiTokenService = {
  /**
   * List all API tokens (admin only)
   */
  async listTokens(): Promise<ApiToken[]> {
    try {
      const response = await apiClient.get('/admin/api-tokens');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list API tokens', { error });
      throw error;
    }
  },

  /**
   * Create a new API token (admin only)
   * Returns the token with the raw token value - only shown once!
   */
  async createToken(request: CreateApiTokenRequest): Promise<ApiTokenCreated> {
    try {
      const response = await apiClient.post('/admin/api-tokens', request);
      return response.data;
    } catch (error) {
      logger.error('Failed to create API token', { error });
      throw error;
    }
  },

  /**
   * Get a single API token by UUID (admin only)
   */
  async getToken(uuid: string): Promise<ApiToken> {
    try {
      const response = await apiClient.get(`/admin/api-tokens/${uuid}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get API token', { error, uuid });
      throw error;
    }
  },

  /**
   * Revoke an API token (admin only)
   */
  async revokeToken(uuid: string): Promise<void> {
    try {
      await apiClient.delete(`/admin/api-tokens/${uuid}`);
    } catch (error) {
      logger.error('Failed to revoke API token', { error, uuid });
      throw error;
    }
  },
};

export default apiTokenService;
