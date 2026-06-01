/**
 * Caller's workspace memberships (`GET /api/me/workspaces`).
 * Powers the shell workspace switcher in the sidebar.
 */
import { defineStore } from 'pinia';
import { computed } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import workspacesService from '@/services/workspacesService';
import type { MyWorkspaceEntry } from '@/types/workspace';
import {
  navigateToWorkspace,
  resolveActiveWorkspaceId,
} from '@/utils/workspaceNavigation';
import { useAuthStore } from '@/stores/auth';

export const MY_WORKSPACES_KEY = ['my-workspaces'] as const;

export const useMyWorkspacesStore = defineStore('myWorkspaces', () => {
  const auth = useAuthStore();
  const queryCache = useQueryCache();

  const query = useQuery({
    key: MY_WORKSPACES_KEY,
    query: () => workspacesService.listMyWorkspaces(),
    enabled: () => auth.isAuthenticated,
    staleTime: 60_000,
  });

  const workspaces = computed<MyWorkspaceEntry[]>(
    () => (Array.isArray(query.data.value) ? query.data.value : []),
  );

  const activeWorkspaceId = computed(() => resolveActiveWorkspaceId(workspaces.value));

  const activeWorkspace = computed<MyWorkspaceEntry | null>(() => {
    const id = activeWorkspaceId.value;
    if (id == null) return workspaces.value[0] ?? null;
    return workspaces.value.find((w) => w.workspace_id === id) ?? workspaces.value[0] ?? null;
  });

  /** Hide the switcher when the operator only belongs to one tenant. */
  const showSwitcher = computed(() => workspaces.value.length > 1);

  const isLoading = computed(
    () => query.status.value === 'pending' && query.data.value === undefined,
  );

  function switchTo(entry: MyWorkspaceEntry): void {
    navigateToWorkspace(entry);
  }

  function reset(): void {
    void queryCache.invalidateQueries({ key: MY_WORKSPACES_KEY });
  }

  return {
    workspaces,
    activeWorkspace,
    activeWorkspaceId,
    showSwitcher,
    isLoading,
    switchTo,
    reset,
    refetch: query.refetch,
  };
});
