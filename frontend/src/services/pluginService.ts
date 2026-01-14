import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  Plugin,
  PluginSetting,
  PluginStorage,
  PluginActivity,
  InstallPluginRequest,
  UpdatePluginRequest,
  SetPluginSettingRequest,
  SetPluginStorageRequest,
  PluginProxyRequest,
  PluginProxyResponse,
} from '@/types/plugin';

/**
 * Plugin Service
 * API client for plugin management and runtime operations
 */
const pluginService = {
  // ===========================================================================
  // Admin Plugin Management
  // ===========================================================================

  /**
   * List all plugins (admin only)
   */
  async listPlugins(): Promise<Plugin[]> {
    try {
      const response = await apiClient.get('/admin/plugins');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list plugins', { error });
      throw error;
    }
  },

  /**
   * Install a new plugin (admin only)
   */
  async installPlugin(request: InstallPluginRequest): Promise<Plugin> {
    try {
      const response = await apiClient.post('/admin/plugins', request);
      return response.data;
    } catch (error) {
      logger.error('Failed to install plugin', { error });
      throw error;
    }
  },

  /**
   * Get a single plugin by UUID (admin only)
   */
  async getPlugin(uuid: string): Promise<Plugin> {
    try {
      const response = await apiClient.get(`/admin/plugins/${uuid}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get plugin', { error, uuid });
      throw error;
    }
  },

  /**
   * Update a plugin (admin only)
   */
  async updatePlugin(uuid: string, request: UpdatePluginRequest): Promise<Plugin> {
    try {
      const response = await apiClient.put(`/admin/plugins/${uuid}`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to update plugin', { error, uuid });
      throw error;
    }
  },

  /**
   * Uninstall a plugin (admin only)
   */
  async uninstallPlugin(uuid: string): Promise<void> {
    try {
      await apiClient.delete(`/admin/plugins/${uuid}`);
    } catch (error) {
      logger.error('Failed to uninstall plugin', { error, uuid });
      throw error;
    }
  },

  /**
   * Upload a plugin bundle (admin only)
   * @param uuid - Plugin UUID
   * @param file - JavaScript bundle file
   */
  async uploadBundle(uuid: string, file: File): Promise<Plugin> {
    try {
      const formData = new FormData();
      formData.append('bundle', file);

      const response = await apiClient.post(`/admin/plugins/${uuid}/bundle`, formData, {
        headers: {
          'Content-Type': 'multipart/form-data',
        },
      });
      return response.data;
    } catch (error) {
      logger.error('Failed to upload plugin bundle', { error, uuid });
      throw error;
    }
  },

  /**
   * Install a plugin from a zip file (admin only)
   * The zip should contain manifest.json and optionally bundle.js
   * @param file - Zip file containing the plugin
   */
  async installFromZip(file: File): Promise<Plugin> {
    try {
      const formData = new FormData();
      formData.append('plugin', file);

      const response = await apiClient.post('/admin/plugins/install', formData, {
        headers: {
          'Content-Type': 'multipart/form-data',
        },
      });
      return response.data;
    } catch (error) {
      logger.error('Failed to install plugin from zip', { error });
      throw error;
    }
  },

  // ===========================================================================
  // Plugin Settings (Admin)
  // ===========================================================================

  /**
   * Get all settings for a plugin (admin only)
   */
  async getPluginSettings(uuid: string): Promise<PluginSetting[]> {
    try {
      const response = await apiClient.get(`/admin/plugins/${uuid}/settings`);
      return response.data || [];
    } catch (error) {
      logger.error('Failed to get plugin settings', { error, uuid });
      throw error;
    }
  },

  /**
   * Set a plugin setting (admin only)
   */
  async setPluginSetting(uuid: string, request: SetPluginSettingRequest): Promise<PluginSetting> {
    try {
      const response = await apiClient.post(`/admin/plugins/${uuid}/settings`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to set plugin setting', { error, uuid, key: request.key });
      throw error;
    }
  },

  /**
   * Delete a plugin setting (admin only)
   */
  async deletePluginSetting(uuid: string, key: string): Promise<void> {
    try {
      await apiClient.delete(`/admin/plugins/${uuid}/settings/${key}`);
    } catch (error) {
      logger.error('Failed to delete plugin setting', { error, uuid, key });
      throw error;
    }
  },

  // ===========================================================================
  // Plugin Activity (Admin)
  // ===========================================================================

  /**
   * Get activity log for a plugin (admin only)
   */
  async getPluginActivity(uuid: string, limit = 50, offset = 0): Promise<PluginActivity[]> {
    try {
      const response = await apiClient.get(`/admin/plugins/${uuid}/activity`, {
        params: { limit, offset },
      });
      return response.data || [];
    } catch (error) {
      logger.error('Failed to get plugin activity', { error, uuid });
      throw error;
    }
  },

  // ===========================================================================
  // Plugin Runtime API (For plugins to use)
  // ===========================================================================

  /**
   * List enabled plugins (for plugin loader)
   */
  async listEnabledPlugins(): Promise<Plugin[]> {
    try {
      const response = await apiClient.get('/plugins/enabled');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list enabled plugins', { error });
      throw error;
    }
  },

  /**
   * Get a storage value for a plugin
   */
  async getStorage(pluginUuid: string, key: string): Promise<PluginStorage> {
    try {
      const response = await apiClient.get(`/plugins/${pluginUuid}/storage/${key}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get plugin storage', { error, pluginUuid, key });
      throw error;
    }
  },

  /**
   * Set a storage value for a plugin
   */
  async setStorage(pluginUuid: string, request: SetPluginStorageRequest): Promise<PluginStorage> {
    try {
      const response = await apiClient.post(`/plugins/${pluginUuid}/storage`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to set plugin storage', { error, pluginUuid, key: request.key });
      throw error;
    }
  },

  /**
   * Delete a storage value for a plugin
   */
  async deleteStorage(pluginUuid: string, key: string): Promise<void> {
    try {
      await apiClient.delete(`/plugins/${pluginUuid}/storage/${key}`);
    } catch (error) {
      logger.error('Failed to delete plugin storage', { error, pluginUuid, key });
      throw error;
    }
  },

  /**
   * Proxy an external request through the backend
   * This allows plugins to make external API calls securely
   */
  async proxyRequest(pluginUuid: string, request: PluginProxyRequest): Promise<PluginProxyResponse> {
    try {
      const response = await apiClient.post(`/plugins/${pluginUuid}/proxy`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to proxy plugin request', { error, pluginUuid, url: request.url });
      throw error;
    }
  },
};

export default pluginService;
