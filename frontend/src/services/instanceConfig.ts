/**
 * Instance-level frontend config (`GET /api/config`).
 *
 * Fetched once at startup so the SPA knows where the selected workspace lives in
 * the URL before it wires up routing. Instance-global (identical for every
 * workspace); per-workspace config lives behind auth.
 */
import apiClient from './apiConfig';
import { logger } from '@/utils/logger';

/** Where the selected workspace lives in the URL. */
export type WorkspaceRouting = 'host' | 'path';

interface InstanceConfig {
  workspace_routing: WorkspaceRouting;
}

// Default 'host': the subdomain / self-hosted model every current deployment
// runs. A pending or failed fetch keeps today's behaviour and never activates
// the single-origin slug-in-path routing.
let workspaceRouting: WorkspaceRouting = 'host';

/**
 * Fetch instance config once at bootstrap. The endpoint is public, so this is
 * safe before auth. Failures are swallowed and leave the 'host' default, so a
 * config blip can never strand the app on a route topology it can't serve.
 */
export async function fetchInstanceConfig(): Promise<void> {
  try {
    const { data } = await apiClient.get<InstanceConfig>('/config');
    if (data?.workspace_routing === 'path' || data?.workspace_routing === 'host') {
      workspaceRouting = data.workspace_routing;
    }
  } catch (e) {
    logger.error('Failed to fetch instance config; defaulting workspace_routing=host', e);
  }
}

/** Where the workspace lives in the URL. 'host' until the config resolves. */
export function getWorkspaceRouting(): WorkspaceRouting {
  return workspaceRouting;
}
