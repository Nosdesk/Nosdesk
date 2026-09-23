import apiClient from '../apiClient';
import type { LicenseOverview, PushMode } from '../types/license';

/**
 * Instance licence and push mode (`backend/src/handlers/admin_license.rs`).
 * Every write answers with the full overview, so callers set the query cache
 * from the response instead of refetching.
 */
const licenseService = {
  async getOverview(): Promise<LicenseOverview> {
    const response = await apiClient.get('/admin/license');
    return response.data;
  },

  async install(key: string): Promise<LicenseOverview> {
    const response = await apiClient.put('/admin/license', { key });
    return response.data;
  },

  async remove(): Promise<LicenseOverview> {
    const response = await apiClient.delete('/admin/license');
    return response.data;
  },

  async setPushMode(mode: PushMode): Promise<LicenseOverview> {
    const response = await apiClient.put('/admin/push-mode', { mode });
    return response.data;
  },

  /** Ask the relay again now, e.g. right after accepting the DPA. */
  async retryRelay(): Promise<LicenseOverview> {
    const response = await apiClient.post('/admin/push-mode/retry');
    return response.data;
  },
};

export default licenseService;
