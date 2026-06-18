/**
 * Caller's workspace memberships (`GET /api/me/workspaces`).
 * Powers the shell workspace switcher in the sidebar.
 */
import { defineStore } from 'pinia';
import { computed } from 'vue';
import { useQuery } from '@pinia/colada';
import workspacesService from '@/services/workspacesService';
import type { MyWorkspaceEntry } from '@/types/workspace';
import { resolveActiveWorkspaceId } from '@/utils/workspaceNavigation';
import { useAuthStore } from '@/stores/auth';
import { getWorkspaceRouting } from '@/services/instanceConfig';
import { activeWorkspaceSlugRef } from '@/services/activeWorkspace';

export const MY_WORKSPACES_KEY = ['my-workspaces'] as const;

export const useMyWorkspacesStore = defineStore('myWorkspaces', () => {
  const auth = useAuthStore();

  const query = useQuery({
    // Account-scoped key: when the signed-in user changes (sign-in,
    // sign-out, account switch) the key changes, so Colada fetches the new
    // account's memberships and caches each account separately — no
    // cross-account leak and no manual reset. `enabled` keeps the
    // signed-out (`anon`) key from fetching, so sign-out never fires a 401.
    key: () => [...MY_WORKSPACES_KEY, auth.user?.uuid ?? 'anon'],
    query: () => workspacesService.listMyWorkspaces(),
    enabled: () => auth.isAuthenticated,
    staleTime: 60_000,
  });

  const workspaces = computed<MyWorkspaceEntry[]>(
    () => (Array.isArray(query.data.value) ? query.data.value : []),
  );

  // The active workspace is whichever one the URL points at, resolved
  // mode-appropriately: in path mode the route slug (the single source of truth
  // the router keeps in `activeWorkspace` and the carrier header read from), and
  // in host mode the subdomain. Both feed one `activeWorkspace`, so the switcher
  // and the carrier never disagree.
  const activeWorkspace = computed<MyWorkspaceEntry | null>(() => {
    const fallback = () => workspaces.value[0] ?? null;
    if (getWorkspaceRouting() === 'path') {
      const slug = activeWorkspaceSlugRef.value;
      if (!slug) return fallback();
      return workspaces.value.find((w) => w.slug === slug) ?? fallback();
    }
    const id = resolveActiveWorkspaceId(workspaces.value);
    if (id == null) return fallback();
    return workspaces.value.find((w) => w.workspace_id === id) ?? fallback();
  });

  const activeWorkspaceId = computed(() => activeWorkspace.value?.workspace_id ?? null);

  /** Hide the switcher when the operator only belongs to one tenant. */
  const showSwitcher = computed(() => workspaces.value.length > 1);

  const isLoading = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  );

  return {
    workspaces,
    activeWorkspace,
    activeWorkspaceId,
    showSwitcher,
    isLoading,
    refetch: query.refetch,
  };
});
