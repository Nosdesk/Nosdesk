import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  Plugin,
  PluginSetting,
  PluginStorage,
  UpdatePluginRequest,
  SetPluginSettingRequest,
  SetPluginStorageRequest,
  PluginProxyRequest,
  PluginProxyResponse,
  CollectionSchemaInfo,
  CollectionRow,
  CollectionListResponse,
  RegistryState,
  InstallFromRegistryRequest,
  SigningOverview,
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
   * Aggregate signing inventory across installed plugins. Drives
   * the trust-tier summary card on the admin list view.
   */
  async getSigningOverview(): Promise<SigningOverview> {
    try {
      const response = await apiClient.get('/admin/plugins/signing-overview');
      return response.data;
    } catch (error) {
      logger.error('Failed to load plugin signing overview', { error });
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

  /**
   * Fetch the registry state. Always returns 200; the response
   * `status` discriminates between snapshot-available, sync-
   * disabled (operator config), sync-pending (boot warm-up), and
   * sync-failed (with reason). The caller branches on `status`
   * to render distinct empty states instead of treating absence
   * as an error.
   */
  async getRegistry(): Promise<RegistryState> {
    const response = await apiClient.get('/admin/plugins/registry');
    return response.data;
  },

  /**
   * Operator-controlled admin-UI flags. Lets the FE render the
   * right surface (e.g. hide the sideload UI when the operator has
   * not enabled it) without trial-and-erroring against gated
   * endpoints.
   */
  async getAdminConfig(): Promise<{ web_sideload_enabled: boolean; registry_enabled: boolean }> {
    const response = await apiClient.get('/admin/plugins/config');
    return response.data;
  },

  /**
   * Force an immediate registry sync. Backs the admin "Retry" button
   * so it actually retries the upstream fetch instead of returning
   * the cached error. Same response shape as `getRegistry`.
   */
  async refreshRegistry(): Promise<RegistryState> {
    const response = await apiClient.post('/admin/plugins/registry/refresh');
    return response.data;
  },

  /**
   * Install a plugin from the registry. The backend resolves the
   * plugin + version, downloads the signed zip, verifies the hash
   * and signature, then upserts via the shared install pipeline.
   */
  async installFromRegistry(request: InstallFromRegistryRequest): Promise<Plugin> {
    const response = await apiClient.post('/admin/plugins/registry/install', request);
    return response.data;
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

  // ===========================================================================
  // Plugin Collections
  // ===========================================================================

  async listCollections(pluginUuid: string): Promise<CollectionSchemaInfo[]> {
    try {
      const response = await apiClient.get(`/plugins/${pluginUuid}/collections`);
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list collections', { error, pluginUuid });
      throw error;
    }
  },

  async getCollectionSchema(pluginUuid: string, name: string): Promise<CollectionSchemaInfo> {
    try {
      const response = await apiClient.get(`/plugins/${pluginUuid}/collections/${name}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get collection schema', { error, pluginUuid, name });
      throw error;
    }
  },

  async listCollectionRows(
    pluginUuid: string,
    name: string,
    params?: { limit?: number; offset?: number; filter?: string; sort_by?: string; sort_order?: string },
  ): Promise<CollectionListResponse> {
    try {
      const response = await apiClient.get(`/plugins/${pluginUuid}/collections/${name}/rows`, { params });
      return response.data;
    } catch (error) {
      logger.error('Failed to list collection rows', { error, pluginUuid, name });
      throw error;
    }
  },

  async createCollectionRow(pluginUuid: string, name: string, data: Record<string, unknown>): Promise<CollectionRow> {
    try {
      const response = await apiClient.post(`/plugins/${pluginUuid}/collections/${name}/rows`, { data });
      return response.data;
    } catch (error) {
      logger.error('Failed to create collection row', { error, pluginUuid, name });
      throw error;
    }
  },

  async getCollectionRow(pluginUuid: string, name: string, rowUuid: string): Promise<CollectionRow> {
    try {
      const response = await apiClient.get(`/plugins/${pluginUuid}/collections/${name}/rows/${rowUuid}`);
      return response.data;
    } catch (error) {
      logger.error('Failed to get collection row', { error, pluginUuid, name, rowUuid });
      throw error;
    }
  },

  async updateCollectionRow(
    pluginUuid: string,
    name: string,
    rowUuid: string,
    data: Record<string, unknown>,
  ): Promise<CollectionRow> {
    try {
      const response = await apiClient.put(`/plugins/${pluginUuid}/collections/${name}/rows/${rowUuid}`, { data });
      return response.data;
    } catch (error) {
      logger.error('Failed to update collection row', { error, pluginUuid, name, rowUuid });
      throw error;
    }
  },

  async deleteCollectionRow(pluginUuid: string, name: string, rowUuid: string): Promise<void> {
    try {
      await apiClient.delete(`/plugins/${pluginUuid}/collections/${name}/rows/${rowUuid}`);
    } catch (error) {
      logger.error('Failed to delete collection row', { error, pluginUuid, name, rowUuid });
      throw error;
    }
  },
};

export default pluginService;
