import apiClient from '../apiClient';
import type { LicenseLink, LicenseOverview, PushMode } from '../types/license';

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

  /** Start connecting to Nosdesk Cloud; the overview carries the code. */
  async startLink(displayHost: string): Promise<LicenseOverview> {
    const response = await apiClient.post('/admin/license/link', { display_host: displayHost });
    return response.data;
  },

  /** The connection in progress, polled while the admin approves it. */
  async getLink(): Promise<{ link: LicenseLink | null; edition: string }> {
    const response = await apiClient.get('/admin/license/link');
    return response.data;
  },

  async cancelLink(): Promise<LicenseOverview> {
    const response = await apiClient.delete('/admin/license/link');
    return response.data;
  },

  /** Ask Nosdesk Cloud for a newer licence now. */
  async refresh(): Promise<LicenseOverview> {
    const response = await apiClient.post('/admin/license/refresh');
    return response.data;
  },

  /** Ask the relay again now, e.g. right after accepting the DPA. */
  async retryRelay(): Promise<LicenseOverview> {
    const response = await apiClient.post('/admin/push-mode/retry');
    return response.data;
  },
};

export default licenseService;
