import type { MyWorkspaceEntry } from '@nosdesk/core/types/workspace';

/** Hostname without port, lowercased. */
function hostLabel(): string {
  return window.location.hostname.toLowerCase();
}

/** First label of a multi-part host (`acme` from `acme.nosdesk.app`). */
function subdomainSlug(): string | null {
  const labels = hostLabel().split('.');
  if (labels.length < 3) return null;
  return labels[0] ?? null;
}

/**
 * Match the caller's current host to one of their workspace
 * memberships. Custom domain wins over subdomain slug.
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

  const slug = subdomainSlug();
  if (slug) {
    const bySlug = workspaces.find((w) => w.slug === slug);
    if (bySlug) return bySlug.workspace_id;
  }

  if (workspaces.length === 1) return workspaces[0].workspace_id;
  return workspaces[0]?.workspace_id ?? null;
}

/**
 * Origin to load when switching into `entry`, preserving the
 * current page path on navigation.
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

  const labels = hostLabel().split('.');
  if (labels.length >= 3) {
    labels[0] = entry.slug;
    const host = labels.join('.');
    const origin = port ? `${protocol}//${host}:${port}` : `${protocol}//${host}`;
    return new URL(path, origin).href;
  }

  // Dev / self-hosted: same origin (single workspace) or
  // `slug.localhost` when the browser supports it.
  if (hostLabel() === 'localhost' || hostLabel() === '127.0.0.1') {
    const devHost = port
      ? `${entry.slug}.${hostLabel()}:${port}`
      : `${entry.slug}.${hostLabel()}`;
    const origin = `${protocol}//${devHost}`;
    return new URL(path, origin).href;
  }

  return new URL(path, window.location.origin).href;
}

export function navigateToWorkspace(entry: MyWorkspaceEntry): void {
  const target = workspaceSwitchUrl(entry);
  if (target === window.location.href) return;
  window.location.assign(target);
}
