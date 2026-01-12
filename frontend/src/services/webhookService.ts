import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  Webhook,
  WebhookCreated,
  CreateWebhookRequest,
  UpdateWebhookRequest,
  WebhookDelivery,
} from '@/types/webhook';

/**
 * Webhook Service
 * Manages webhooks for external integrations
 */
const webhookService = {
  /**
   * List all webhooks (admin only)
   */
  async listWebhooks(): Promise<Webhook[]> {
    try {
      const response = await apiClient.get('/admin/webhooks');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list webhooks', { error });
      throw error;
    }
  },

  /**
   * Create a new webhook (admin only)
   * Returns the webhook with the raw secret - only shown once!
   */
  async createWebhook(request: CreateWebhookRequest): Promise<WebhookCreated> {
    try {
      const response = await apiClient.post('/admin/webhooks', request);
      return response.data;
    } catch (error) {
      logger.error('Failed to create webhook', { error });
      throw error;
    }
  },

  /**
   * Get available event types
   */
  async getEventTypes(): Promise<string[]> {
    try {
      const response = await apiClient.get('/admin/webhooks/event-types');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to get event types', { error });
      throw error;
    }
  },

  /**
   * Get a single webhook by UUID (admin only)
   */
  async getWebhook(uuid: string): Promise<Webhook> {
    try {
      const response = await apiClient.get(`/admin/webhooks/${uuid}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get webhook', { error, uuid });
      throw error;
    }
  },

  /**
   * Update a webhook (admin only)
   */
  async updateWebhook(uuid: string, request: UpdateWebhookRequest): Promise<Webhook> {
    try {
      const response = await apiClient.put(`/admin/webhooks/${uuid}`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to update webhook', { error, uuid });
      throw error;
    }
  },

  /**
   * Delete a webhook (admin only)
   */
  async deleteWebhook(uuid: string): Promise<void> {
    try {
      await apiClient.delete(`/admin/webhooks/${uuid}`);
    } catch (error) {
      logger.error('Failed to delete webhook', { error, uuid });
      throw error;
    }
  },

  /**
   * Get delivery history for a webhook
   */
  async getDeliveries(uuid: string, limit = 50, offset = 0): Promise<WebhookDelivery[]> {
    try {
      const response = await apiClient.get(`/admin/webhooks/${uuid}/deliveries`, {
        params: { limit, offset },
      });
      return response.data || [];
    } catch (error) {
      logger.error('Failed to get webhook deliveries', { error, uuid });
      throw error;
    }
  },

  /**
   * Send a test event to a webhook
   */
  async testWebhook(uuid: string): Promise<void> {
    try {
      await apiClient.post(`/admin/webhooks/${uuid}/test`);
    } catch (error) {
      logger.error('Failed to test webhook', { error, uuid });
      throw error;
    }
  },
};

export default webhookService;
