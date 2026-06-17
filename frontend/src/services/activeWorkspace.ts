/**
 * The active workspace slug, as a plain module value.
 *
 * Decoupled on purpose (no imports): the router keeps it in sync on navigation
 * and the axios interceptor reads it to set the `X-Nosdesk-Workspace` header,
 * without either side importing the other (which would cycle through the store
 * and router). Only set in path mode; `null` in host mode means no header is
 * sent and the backend resolves the workspace from the Host, as today.
 */
let slug: string | null = null;

export function setActiveWorkspaceSlug(next: string | null): void {
  slug = next;
}

export function activeWorkspaceSlug(): string | null {
  return slug;
}
