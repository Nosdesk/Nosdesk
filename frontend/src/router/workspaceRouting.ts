/**
 * Slug-in-path workspace routing (Model C, increment 3, stage 2b).
 *
 * The single-origin agent app puts the selected workspace in the URL path
 * (`app.nosdesk.com/acme/tickets/123`), Linear-style, while self-hosted /
 * subdomain installs keep bare paths (`/tickets/123`). Both come from ONE named
 * route per page: each authenticated route gets an *optional* leading
 * `/:workspace?` segment, so the same record matches the bare and the
 * slug-prefixed URL. `route.name`, children, and meta all work unchanged in both
 * modes, so nothing downstream has to special-case the routing.
 *
 * How a navigation flows in `path` mode:
 *   router.push('/tickets/123')  ->  resolves to /:workspace?/tickets/:id with
 *     the workspace absent  ->  the guard prefixes it with the workspace of the
 *     route you're currently on  ->  /acme/tickets/123.
 * Every `router.push`, named push, and string `<RouterLink>` resolves to the
 * same record, so the one guard covers all ~170 call sites untouched. In `host`
 * mode the optional segment must never be exercised: a URL that resolved a
 * workspace slug is a stray path and is 404'd, preserving today's behaviour.
 *
 * Both shapes come from one route table, so this needs no async config at
 * router-creation time; the routing *mode* is read at runtime by the guard.
 */
import type {
  RouteRecordRaw,
  Router,
  RouteLocationNormalized,
} from 'vue-router';
import { getWorkspaceRouting } from '@/services/instanceConfig';

const WORKSPACE_PARAM = 'workspace';

/** Authenticated app routes are workspace-scoped; public routes (login, auth
 *  callbacks, guest portal) and the catch-all stay bare. Routes default to
 *  authenticated unless they opt out with `requiresAuth: false`. */
function isWorkspaceScoped(r: RouteRecordRaw): boolean {
  const requiresAuth = (r.meta?.requiresAuth ?? true) !== false;
  const isCatchAll = typeof r.path === 'string' && r.path.includes(':pathMatch');
  return requiresAuth && !isCatchAll;
}

/**
 * Give every authenticated route an optional leading `/:workspace?` so one
 * named record serves both the bare and the slug-prefixed URL. The home route
 * (`/`) becomes `/:workspace?`; everything else is prefixed in place. Children
 * are relative, so they ride along under both forms automatically.
 */
export function withWorkspaceRouting(routes: RouteRecordRaw[]): RouteRecordRaw[] {
  return routes.map((r) => {
    if (!isWorkspaceScoped(r)) return r;
    const path =
      r.path === '/' ? `/:${WORKSPACE_PARAM}?` : `/:${WORKSPACE_PARAM}?${r.path}`;
    return { ...r, path };
  });
}

/** The workspace slug carried by a route, or null. */
export function workspaceSlugOf(route: RouteLocationNormalized): string | null {
  const slug = route.params[WORKSPACE_PARAM];
  return typeof slug === 'string' && slug ? slug : null;
}

function isAuthed(route: RouteLocationNormalized): boolean {
  return route.matched.some((r) => (r.meta?.requiresAuth ?? true) !== false);
}

/**
 * Workspace URL guard. Register before the auth guards so they evaluate the
 * final route (a redirect re-runs the chain anyway, but ordering keeps it to
 * one pass).
 *
 * - **host mode**: the optional segment must never be exercised. A URL that
 *   resolved a workspace slug is a stray path; 404 it, matching the behaviour
 *   from before slug routing existed. Self-hosted thus never shows a slug.
 * - **path mode**: a bare authenticated path is prefixed with the workspace of
 *   the route you're currently on (`from`), so `/tickets/5` clicked while on
 *   `/acme/...` becomes `/acme/tickets/5`. A bare entry with no `from` workspace
 *   (a deep link's first hop) passes through; the post-login landing (stage 5)
 *   routes it to a concrete workspace.
 */
export function installWorkspaceGuard(router: Router): void {
  router.beforeEach((to, from) => {
    const slug = workspaceSlugOf(to);
    if (getWorkspaceRouting() !== 'path') {
      return slug ? { path: '/error/404', replace: true } : true;
    }
    if (slug) return true;
    if (!isAuthed(to)) return true;
    const fromSlug = workspaceSlugOf(from);
    if (!fromSlug) return true;
    return { path: `/${fromSlug}${to.fullPath}`, replace: true };
  });
}
