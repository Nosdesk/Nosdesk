import type { MyWorkspaceEntry } from '@nosdesk/core/types/workspace';

/** Hostname without port, lowercased. */
function hostLabel(): string {
  return window.location.hostname.toLowerCase();
}

/**
 * Match the caller's current host to one of their workspace memberships.
 *
 * The hosted agent app is a single central origin (Model C:
 * `app.nosdesk.dev/<slug>`), so the workspace is selected by path/header,
 * not by a per-tenant subdomain. This resolver only covers the host-mode
 * cases that remain: a verified custom domain (its own origin) and the
 * single-workspace self-hosted deployment.
 */
export function resolveActiveWorkspaceId(
  workspaces: MyWorkspaceEntry[],
): number | null {
  if (workspaces.length === 0) return null;

  const host = hostLabel();
  const byDomain = workspaces.find(
    (w) => w.custom_domain?.toLowerCase() === host,
  );
  if (byDomain) return byDomain.workspace_id;

  // Self-hosted single-workspace deployment: the sole workspace is active.
  if (workspaces.length === 1) return workspaces[0].workspace_id;
  return workspaces[0]?.workspace_id ?? null;
}

/**
 * Origin to load when switching into `entry`, preserving the current page
 * path. A verified custom domain is a distinct origin; otherwise the switch
 * stays on the current origin (the central agent app switches workspace
 * in-app via the path, not by navigating to another origin).
 */
export function workspaceSwitchUrl(
  entry: MyWorkspaceEntry,
  path = window.location.pathname + window.location.search,
): string {
  const { protocol, port } = window.location;

  if (entry.custom_domain) {
    const origin = port
      ? `${protocol}//${entry.custom_domain}:${port}`
      : `${protocol}//${entry.custom_domain}`;
    return new URL(path, origin).href;
  }

  return new URL(path, window.location.origin).href;
}

export function navigateToWorkspace(entry: MyWorkspaceEntry): void {
  const target = workspaceSwitchUrl(entry);
  if (target === window.location.href) return;
  window.location.assign(target);
}
