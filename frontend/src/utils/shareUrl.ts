/**
 * Build a full, shareable URL for a named app route.
 *
 * In path mode the workspace slug is folded in (so a copied link opens the right
 * tenant for whoever receives it); in host mode the origin already carries the
 * tenant (subdomain) so no slug is added. Built via `router.resolve` so the URL
 * inherits the route table rather than re-deriving the path shape in a second
 * place.
 */
import router from '@/router';
import { activeWorkspaceSlug } from '@/services/activeWorkspace';
import { getWorkspaceRouting } from '@nosdesk/core/services/instanceConfig';

export function shareableRouteUrl(
  name: string,
  params: Record<string, string>,
): string {
  const workspace =
    getWorkspaceRouting() === 'path' ? activeWorkspaceSlug() : null;
  const { href } = router.resolve({
    name,
    params: workspace ? { ...params, workspace } : params,
  });
  return `${window.location.origin}${href}`;
}
