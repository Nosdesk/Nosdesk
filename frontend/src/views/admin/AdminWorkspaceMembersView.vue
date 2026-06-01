<script setup lang="ts">
/**
 * Per-workspace member management UI (Phase 4 W3).
 *
 * STATUS: scaffold only. Data layer wired via Pinia Colada useQuery
 * + workspacesService. Template is a minimal placeholder for Cursor
 * to flesh out — reference ApiTokensView.vue for the polished pattern.
 *
 * Capabilities to surface:
 *   * List the workspace's members + roles.
 *   * Invite a user (by user_uuid for now — pair with a user-picker
 *     component in the final UI, see useUserPicker.ts).
 *   * Change a member's role (owner / admin / agent / member).
 *   * Remove a member.
 *
 * Backend safety invariants the UI should respect / surface:
 *   * The last owner can't be removed or demoted — backend returns
 *     409 `last_owner`. Catch this and show a clear "promote another
 *     member first" hint rather than a generic error.
 *   * Re-adding an existing member is idempotent (200 with status:
 *     'already_member'); surface this gracefully if it happens.
 *   * Archived workspaces refuse new member additions (404 from
 *     find_by_id with archived filter). The workspace switcher above
 *     already hides archived rows, so this is mostly defensive.
 */
import { computed, ref } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import workspacesService from '@/services/workspacesService';
import type { WorkspaceMember, WorkspaceRole } from '@/types/workspace';

const fluent = useFluent();
// eslint-disable-next-line @typescript-eslint/no-unused-vars
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const route = useRoute();
const workspaceId = computed(() => Number(route.params.id));

const MEMBERS_KEY = computed(
  () => ['admin-workspace-members', workspaceId.value] as const,
);
const queryCache = useQueryCache();
const membersQuery = useQuery({
  key: MEMBERS_KEY,
  query: () => workspacesService.listMembers(workspaceId.value),
});
const members = computed<WorkspaceMember[]>(() =>
  Array.isArray(membersQuery.data.value) ? membersQuery.data.value : [],
);
const isFirstLoad = computed(
  () =>
    membersQuery.status.value === 'pending' &&
    membersQuery.data.value === undefined,
);
const loadError = computed(() =>
  membersQuery.error.value ? 'Failed to load members' : '',
);

const isSaving = ref(false);
const errorMessage = ref('');

const refresh = () => queryCache.invalidateQueries({ key: MEMBERS_KEY.value });

async function addMember(userUuid: string, role: WorkspaceRole) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.addMember(workspaceId.value, {
      user_uuid: userUuid,
      role,
    });
    refresh();
  } catch (e: unknown) {
    errorMessage.value = (e as Error).message ?? 'Add member failed';
  } finally {
    isSaving.value = false;
  }
}

async function changeRole(userUuid: string, role: WorkspaceRole) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.updateMemberRole(workspaceId.value, userUuid, {
      role,
    });
    refresh();
  } catch (e: unknown) {
    // Backend returns 409 `last_owner` if this would demote the only
    // owner. Cursor: detect that response shape (axios error.response.data.error)
    // and surface a meaningful message instead of the generic one.
    errorMessage.value = (e as Error).message ?? 'Change role failed';
  } finally {
    isSaving.value = false;
  }
}

async function removeMember(userUuid: string) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.removeMember(workspaceId.value, userUuid);
    refresh();
  } catch (e: unknown) {
    // Same 409 last_owner story as changeRole — surface specifically.
    errorMessage.value = (e as Error).message ?? 'Remove member failed';
  } finally {
    isSaving.value = false;
  }
}

defineExpose({ addMember, changeRole, removeMember });
</script>

<template>
  <div class="p-6">
    <header class="flex items-center gap-4 mb-6">
      <router-link :to="{ name: 'admin-workspaces' }" class="text-secondary">
        &larr; Workspaces
      </router-link>
      <h1 class="text-xl font-semibold">Members</h1>
    </header>

    <div v-if="isFirstLoad" class="text-secondary">Loading…</div>
    <div v-else-if="loadError" class="text-status-error">{{ loadError }}</div>
    <div v-else-if="members.length === 0" class="text-secondary">
      No members yet.
    </div>
    <table v-else class="w-full text-sm">
      <thead>
        <tr class="text-left">
          <th>User UUID</th>
          <th>Role</th>
          <th>Invited</th>
          <th>Accepted</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="m in members" :key="m.user_uuid">
          <td>{{ m.user_uuid }}</td>
          <td>{{ m.role }}</td>
          <td>{{ m.invited_at }}</td>
          <td>{{ m.accepted_at ?? '—' }}</td>
          <td>
            <!-- TODO (Cursor): role-picker + remove button.
                 Pair with the user-picker pattern from useUserPicker.ts
                 for the invite form. Show a clear 'cannot remove the
                 only owner' state when 409 last_owner comes back. -->
            <button :disabled="isSaving">…</button>
          </td>
        </tr>
      </tbody>
    </table>

    <div v-if="errorMessage" class="text-status-error mt-4">
      {{ errorMessage }}
    </div>
  </div>
</template>
