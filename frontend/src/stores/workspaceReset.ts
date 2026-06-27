/**
 * Tear down all *workspace-scoped* client state.
 *
 * One routine shared by two callers:
 *   - `logout()` (account/session teardown layers this on top), and
 *   - the in-app workspace switch (Model C), which resets + re-hydrates
 *     instead of reloading to another subdomain.
 *
 * Scope boundary: this resets state that belongs to the *current workspace*
 * (config stores, the sync pool, SSE, collab, cached queries). It deliberately
 * does NOT touch account/session state (the signed-in user, cookies, the
 * device-level theme, recent tickets), which is account-scoped and survives a
 * workspace switch — `logout()` owns clearing those. Branding is also out of
 * scope on purpose: logout keeps the current workspace's brand so the login
 * page stays branded, and the switch re-fetches the new workspace's brand
 * directly (no flash to default), so it is re-applied at each hydration
 * boundary rather than reset here.
 *
 * Modules are imported dynamically (like `logout()`), keeping load order loose
 * and avoiding static import cycles with the stores this touches.
 */
import { logger } from '@nosdesk/core/utils/logger';

export async function resetWorkspaceScopedState(): Promise<void> {
  // Config stores that cache a slow-moving, workspace-scoped set. Reset
  // independently so one failure doesn't skip the rest.
  const storeResets = await Promise.allSettled([
    import('@nosdesk/core/stores/featureFlags').then((m) => m.useFeatureFlagsStore().reset()),
    import('@nosdesk/core/stores/workflowStates').then((m) => m.useWorkflowStatesStore().reset()),
    import('@/composables/useWorkspaceCapabilities').then((m) =>
      m.resetWorkspaceCapabilities(),
    ),
    import('@nosdesk/core/stores/cycles').then((m) => m.useCyclesStore().reset()),
    import('@/stores/savedViews').then((m) => m.useSavedViewsStore().reset()),
  ]);
  for (const r of storeResets) {
    if (r.status === 'rejected') {
      logger.error('Failed to reset a workspace-scoped store', r.reason);
    }
  }

  // Pinia Colada query cache. Drop every entry rather than invalidate: an
  // invalidated-but-not-removed entry keeps the previous workspace's data, which
  // a cache-first read on the next workspace would serve before refetching. v1.2
  // has no clear(), so remove entries explicitly.
  try {
    const { useQueryCache } = await import('@pinia/colada');
    const cache = useQueryCache();
    for (const entry of cache.getEntries()) {
      cache.remove(entry);
    }
  } catch (e) {
    logger.error('Failed to clear the query cache', e);
  }

  // Drop the selected-workspace slug so the axios interceptor stops sending a
  // stale `X-Nosdesk-Workspace` header; the next workspace navigation re-sets it.
  try {
    const { setActiveWorkspaceSlug } = await import('@/services/activeWorkspace');
    setActiveWorkspaceSlug(null);
  } catch (e) {
    logger.error('Failed to clear the active workspace slug', e);
  }

  // Sync runtime, SSE bridge, and collab IndexedDB. The sync pool's IDB is keyed
  // by (user, schema) with no workspace today, so a full teardown is the only
  // way to keep one workspace's rows from bleeding into the next; re-hydration
  // is the caller's job.
  try {
    const [{ tearDown }, { detachSseBridge }, { purgeAllCollabDocs }] = await Promise.all([
      import('@/sync/lifecycle'),
      import('@/sync/sseBridge'),
      import('@/utils/collabLocalCache'),
    ]);
    detachSseBridge();
    await tearDown();
    await purgeAllCollabDocs();
  } catch (e) {
    logger.error('Failed to tear down the sync runtime', e);
  }
}
