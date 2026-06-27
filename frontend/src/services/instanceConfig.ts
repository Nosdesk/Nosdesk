/**
 * Instance-level frontend config (`GET /api/config`).
 *
 * Fetched once at startup so the SPA knows where the selected workspace lives in
 * the URL before it wires up routing. Instance-global (identical for every
 * workspace); per-workspace config lives behind auth.
 */
import apiClient from '@nosdesk/core/apiClient';
import { logger } from '@nosdesk/core/utils/logger';

/** Where the selected workspace lives in the URL. */
export type WorkspaceRouting = 'host' | 'path';

/** Whether this instance is the managed hosted SaaS or a self-hosted install. */
export type DeploymentMode = 'hosted' | 'self_hosted';

interface InstanceConfig {
  workspace_routing: WorkspaceRouting;
  deployment_mode: DeploymentMode;
  inbound_forwarding_enabled: boolean;
}

// Default 'host': the subdomain / self-hosted model every current deployment
// runs. A pending or failed fetch keeps today's behaviour and never activates
// the single-origin slug-in-path routing.
let workspaceRouting: WorkspaceRouting = 'host';

// Default 'self_hosted': the conservative, backward-compatible default. Hosted-
// aware UI that hides infrastructure must still rely on backend enforcement (the
// API redacts platform secrets regardless of this client hint), so a config blip
// can never leak.
let deploymentMode: DeploymentMode = 'self_hosted';

// Default false: forwarding-based inbound email needs an instance inbound
// domain (the hosted SES-receiving path), absent on self-host. The admin UI
// hides the forwarding channel type until the config confirms it's available.
let inboundForwardingEnabled = false;

// Memoised so bootstrap and the router guard share one fetch. The guard awaits
// this before reading the routing mode, so a cold load (hard refresh, deep link)
// resolves the mode BEFORE the workspace guard decides. Otherwise a slugged URL
// is judged in the 'host' default and wrongly 404'd while the fetch is in flight.
let configPromise: Promise<void> | null = null;

/**
 * Fetch instance config once at bootstrap. The endpoint is public, so this is
 * safe before auth. Failures are swallowed and leave the 'host' default, so a
 * config blip can never strand the app on a route topology it can't serve.
 * Idempotent: repeat calls return the same in-flight/settled promise.
 */
export function fetchInstanceConfig(): Promise<void> {
  if (!configPromise) {
    configPromise = (async () => {
      try {
        const { data } = await apiClient.get<InstanceConfig>('/config');
        if (data?.workspace_routing === 'path' || data?.workspace_routing === 'host') {
          workspaceRouting = data.workspace_routing;
        }
        if (data?.deployment_mode === 'hosted' || data?.deployment_mode === 'self_hosted') {
          deploymentMode = data.deployment_mode;
        }
        if (typeof data?.inbound_forwarding_enabled === 'boolean') {
          inboundForwardingEnabled = data.inbound_forwarding_enabled;
        }
      } catch (e) {
        logger.error('Failed to fetch instance config; defaulting workspace_routing=host', e);
      }
    })();
  }
  return configPromise;
}

/** Where the workspace lives in the URL. 'host' until the config resolves. */
export function getWorkspaceRouting(): WorkspaceRouting {
  return workspaceRouting;
}

/** The instance's deployment mode. 'self_hosted' until the config resolves. */
export function getDeploymentMode(): DeploymentMode {
  return deploymentMode;
}

/** True on the managed hosted SaaS. Use to hide operator/infra-only admin UI. */
export function isHostedDeployment(): boolean {
  return deploymentMode === 'hosted';
}

/** True when forwarding-based inbound email is available on this instance. */
export function isInboundForwardingEnabled(): boolean {
  return inboundForwardingEnabled;
}
