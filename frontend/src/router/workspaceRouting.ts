/**
 * Slug-in-path workspace routing (Model C, increment 3, stage 2b).
 *
 * The single-origin agent app puts the selected workspace in the URL path
 * (`app.nosdesk.com/acme/tickets/123`), Linear-style. We get there without
 * touching the ~170 navigation call sites by keeping every route flat AND
 * registering the authenticated ones a second time under a `/:workspace`
 * parent (dual-mount), then redirecting bare authenticated paths onto the
 * prefixed copy with a single guard.
 *
 * How a navigation flows in `path` mode:
 *   router.push('/tickets/123')  ->  resolves to the FLAT /tickets/:id route
 *     ->  prefix guard sees a flat authed route with no `workspace` param
 *     ->  redirects to /<activeSlug>/tickets/123
 *     ->  matches the nested copy and renders.
 * Named pushes and string `<RouterLink>`s resolve to the same flat routes, so
 * the one guard covers all of them. In `host` mode the guard is inert and the
 * flat routes serve directly, exactly as today.
 *
 * Bootstrap note: both shapes are registered unconditionally, so this needs no
 * async config at router-creation time. The routing *mode* is read at runtime
 * by the guard, by which point `/api/config` has resolved.
 */
import type {
  RouteRecordRaw,
  Router,
  RouteLocationNormalized,
} from 'vue-router';
import { getWorkspaceRouting } from '@/services/instanceConfig';

const WORKSPACE_PARAM = 'workspace';

/** Authenticated app routes are workspace-scoped; public routes (login, auth
 *  callbacks, guest portal) and the catch-all stay un-prefixed. Routes default
 *  to authenticated unless they opt out with `requiresAuth: false`. */
function isWorkspaceScoped(r: RouteRecordRaw): boolean {
  const requiresAuth = (r.meta?.requiresAuth ?? true) !== false;
  const isCatchAll = typeof r.path === 'string' && r.path.includes(':pathMatch');
  return requiresAuth && !isCatchAll;
}

/** A nested copy of a flat route: same component/meta/children, path made
 *  relative, and the name dropped (names live on the flat copies; duplicate
 *  names are a Vue Router error). Navigation resolves to the named flat route
 *  and the guard redirects it onto this copy. The flat name is preserved in
 *  `meta.routeName` so logical-page checks survive the rename to anonymous
 *  (read it via `effectiveRouteName`). */
function toNestedChild(r: RouteRecordRaw): RouteRecordRaw {
  const child = {
    ...r,
    path: r.path.replace(/^\//, ''),
    meta: { ...r.meta, routeName: r.name },
  } as RouteRecordRaw;
  delete (child as { name?: unknown }).name;
  return child;
}

/** Logical page name regardless of dual-mount: the route record's `name` on a
 *  flat route, or the preserved `meta.routeName` on a nested copy. Use this
 *  instead of `route.name` anywhere a check must hold in both routing modes. */
export function effectiveRouteName(route: RouteLocationNormalized): string | null {
  if (typeof route.name === 'string' && route.name) return route.name;
  const metaName = route.meta?.routeName;
  return typeof metaName === 'string' && metaName ? metaName : null;
}

/**
 * Dual-mount the route table: flat routes as-is (host mode), plus the
 * authenticated ones under a component-less `/:workspace` parent (path mode).
 * The parent has no component, so its children render in App.vue's
 * `<router-view>` exactly as the flat copies do.
 */
export function withWorkspaceRouting(routes: RouteRecordRaw[]): RouteRecordRaw[] {
  const nested: RouteRecordRaw = {
    path: `/:${WORKSPACE_PARAM}`,
    // Inert in host mode. The nested copies only ever exist to serve `path`
    // mode; in `host` mode (self-hosted / subdomain) a stray slug-shaped URL
    // must 404 exactly as the catch-all does, never render a nested copy. This
    // is what guarantees a single-workspace self-hosted instance never shows a
    // slug in the URL: there is no reachable nested route there at all.
    beforeEnter: () =>
      getWorkspaceRouting() === 'path' ? true : { path: '/error/404' },
    children: routes.filter(isWorkspaceScoped).map(toNestedChild),
  };
  return [...routes, nested];
}

/** The workspace slug carried by a route, or null. */
export function workspaceSlugOf(route: RouteLocationNormalized): string | null {
  const slug = route.params[WORKSPACE_PARAM];
  return typeof slug === 'string' && slug ? slug : null;
}

/**
 * Redirect bare authenticated paths onto their `/:workspace`-prefixed copy when
 * routing in `path` mode. Inert in `host` mode. Register before the auth guards
 * so they evaluate the final, prefixed route (a redirect re-runs the chain
 * anyway, but ordering keeps it to one pass).
 *
 * The slug to prefix with comes from the route you're *currently* on (`from`):
 * a link to `/tickets/5` clicked while on `/acme/...` becomes `/acme/tickets/5`.
 * When `from` carries no workspace (a deep link's first navigation, or host->
 * path transition) the navigation passes through unprefixed; the post-login
 * landing (stage 5) is what routes a bare entry to a concrete workspace.
 */
export function installWorkspacePrefixGuard(router: Router): void {
  router.beforeEach((to, from) => {
    if (getWorkspaceRouting() !== 'path') return true;
    // Already on a nested (prefixed) route.
    if (workspaceSlugOf(to)) return true;
    // Only authenticated app routes get a workspace; public routes stay bare.
    const requiresAuth = to.matched.some(
      (r) => (r.meta?.requiresAuth ?? true) !== false,
    );
    if (!requiresAuth) return true;
    const slug = workspaceSlugOf(from);
    if (!slug) return true;
    return { path: `/${slug}${to.fullPath}`, replace: true };
  });
}
