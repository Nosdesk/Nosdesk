/**
 * Instance-level frontend config (`GET /api/config`).
 *
 * Fetched once at startup so the SPA knows where the selected workspace lives in
 * the URL before it wires up routing. Instance-global (identical for every
 * workspace); per-workspace config lives behind auth.
 */
import { readonly, ref, type Ref } from 'vue';
import apiClient from '../apiClient';
import { logger } from '../utils/logger';

/** Where the selected workspace lives in the URL. */
export type WorkspaceRouting = 'host' | 'path';

/** Whether this instance is the managed hosted SaaS or a self-hosted install. */
export type DeploymentMode = 'hosted' | 'self_hosted';

interface InstanceConfig {
  workspace_routing: WorkspaceRouting;
  deployment_mode: DeploymentMode;
  inbound_forwarding_enabled: boolean;
  /** Control-plane dashboard base URL (hosted mode); '' when unset. */
  control_plane_url: string;
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

// Control-plane dashboard base URL, hosted mode only. Empty by default and
// until the config resolves; the hosted "add member" hand-off renders its link
// only when this is non-empty (unset -> a plain explainer, never a dead form).
let controlPlaneUrl = '';

// Memoised so bootstrap and the router guard share one fetch. The guard awaits
// this before reading the routing mode, so a cold load (hard refresh, deep link)
// resolves the mode BEFORE the workspace guard decides. Otherwise a slugged URL
// is judged in the 'host' default and wrongly 404'd while the fetch is in flight.
let configPromise: Promise<void> | null = null;

// Reactive latch: flips true once the fetch SUCCEEDS (a failure clears the memo
// to retry, see fetchInstanceConfig). Lets the workspace-ready gate tell "still
// the 'host' default because we haven't resolved yet" from a real answer, so it
// never acts on the pre-fetch default. Owned here (the fetch's owner) so nothing
// fires a premature /config request just to observe completion.
const configResolved = ref(false);

/**
 * Fetch instance config once per server. The endpoint is public, so it's safe
 * before auth. Idempotent: repeat calls share one in-flight/settled promise. A
 * failed fetch clears the memo so the next call retries instead of latching the
 * 'host' default; resetInstanceConfig() re-arms it when the server changes.
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
        if (typeof data?.control_plane_url === 'string') {
          controlPlaneUrl = data.control_plane_url;
        }
        configResolved.value = true;
      } catch (e) {
        logger.error('Failed to fetch instance config; will retry on next call', e);
        // A failed fetch must NOT latch the 'host' default for the session. Clear
        // the memo so the next caller (e.g. the router guard, once the mobile
        // base URL is configured) retries. Leaving configResolved false keeps
        // workspace-ready gates closed rather than firing header-less.
        configPromise = null;
      }
    })();
  }
  return configPromise;
}

/**
 * Forget the cached instance config so the next {@link fetchInstanceConfig}
 * re-resolves it. The config is SERVER-SPECIFIC (routing topology differs per
 * instance), so the mobile transport calls this whenever it points at a new
 * server: without it, a value fetched against the default/bootstrap server (or a
 * pre-server-selection failed fetch) would leak across and strand the app in the
 * wrong routing mode for the whole session.
 */
export function resetInstanceConfig(): void {
  configPromise = null;
  configResolved.value = false;
  workspaceRouting = 'host';
  deploymentMode = 'self_hosted';
  inboundForwardingEnabled = false;
  controlPlaneUrl = '';
}

/** Reactive: `true` once {@link fetchInstanceConfig} has settled (success or
 *  failure). Consumers gating on the routing mode read this so they don't act
 *  on the pre-fetch 'host' default. */
export const instanceConfigResolvedRef: Readonly<Ref<boolean>> = readonly(configResolved);

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

/** Control-plane dashboard base URL (hosted mode); '' when unset. Consumers
 *  render a hand-off link only when non-empty. */
export function getControlPlaneUrl(): string {
  return controlPlaneUrl;
}
