<script setup lang="ts">
/**
 * Admin workspace lifecycle UI (Phase 4 W1).
 *
 * STATUS: scaffold only. The data layer is wired to
 * `workspacesService` via Pinia Colada `useQuery` per the repo's
 * data-fetching pattern (see ApiTokensView.vue for the reference).
 * The template is intentionally minimal — flesh out modals,
 * confirmations, and styling to match the rest of the admin surface.
 *
 * Capabilities to surface in the final UI:
 *   * List workspaces (toggle active-only vs include-archived).
 *   * Create workspace (slug + name; slug validated server-side).
 *   * Rename workspace (name only; slug is immutable).
 *   * Archive / restore.
 *   * Hard-delete with `?confirm=<slug>` typed-confirmation modal.
 *
 * Backend gating: handlers run on require_admin today; W2 will swap
 * to require_platform_admin. The route's adminRequired meta flag
 * already gates this view client-side.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import workspacesService from '@/services/workspacesService';
import type { Workspace } from '@/types/workspace';

const fluent = useFluent();
// eslint-disable-next-line @typescript-eslint/no-unused-vars
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const includeArchived = ref(false);

const WORKSPACES_KEY = computed(
  () => ['admin-workspaces', includeArchived.value] as const,
);
const queryCache = useQueryCache();
const workspacesQuery = useQuery({
  key: WORKSPACES_KEY,
  query: () => workspacesService.list(includeArchived.value),
});
const workspaces = computed<Workspace[]>(() =>
  Array.isArray(workspacesQuery.data.value) ? workspacesQuery.data.value : [],
);
const isFirstLoad = computed(
  () =>
    workspacesQuery.status.value === 'pending' &&
    workspacesQuery.data.value === undefined,
);
const loadError = computed(() =>
  workspacesQuery.error.value ? 'Failed to load workspaces' : '',
);

// ---- Mutations -----------------------------------------------------
//
// Cursor: each of these should invalidate WORKSPACES_KEY on success so
// the list re-renders, plus surface a toast / inline confirmation per
// the admin-page UX patterns elsewhere. See ApiTokensView for the
// `queryCache.invalidateQueries({ key: ... })` shape.

const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const refreshList = () =>
  queryCache.invalidateQueries({ key: WORKSPACES_KEY.value });

async function createWorkspace(payload: { slug: string; name: string }) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    const created = await workspacesService.create(payload);
    successMessage.value = `Workspace ${created.slug} created`;
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = (e as Error).message ?? 'Create failed';
  } finally {
    isSaving.value = false;
  }
}

async function archiveWorkspace(id: number) {
  isSaving.value = true;
  try {
    await workspacesService.archive(id);
    refreshList();
  } finally {
    isSaving.value = false;
  }
}

async function restoreWorkspace(id: number) {
  isSaving.value = true;
  try {
    await workspacesService.restore(id);
    refreshList();
  } finally {
    isSaving.value = false;
  }
}

async function renameWorkspace(id: number, name: string) {
  isSaving.value = true;
  try {
    await workspacesService.rename(id, { name });
    refreshList();
  } finally {
    isSaving.value = false;
  }
}

async function hardDeleteWorkspace(id: number, confirmSlug: string) {
  isSaving.value = true;
  try {
    await workspacesService.hardDelete(id, confirmSlug);
    refreshList();
  } finally {
    isSaving.value = false;
  }
}

// Exposed for the template — the unused vars are intentional placeholders
// so Cursor knows the contract for the button handlers it wires up.
defineExpose({
  createWorkspace,
  archiveWorkspace,
  restoreWorkspace,
  renameWorkspace,
  hardDeleteWorkspace,
});
</script>

<template>
  <div class="p-6">
    <header class="flex items-center gap-4 mb-6">
      <h1 class="text-xl font-semibold">Workspaces</h1>
      <label class="ml-auto flex items-center gap-2 text-sm">
        <input type="checkbox" v-model="includeArchived" />
        Include archived
      </label>
    </header>

    <!-- TODO (Cursor): replace with the real Skeleton + EmptyState +
         table components from frontend/src/components/common. Pattern
         to mirror: ApiTokensView.vue. -->
    <div v-if="isFirstLoad" class="text-secondary">Loading…</div>
    <div v-else-if="loadError" class="text-status-error">{{ loadError }}</div>
    <div v-else-if="workspaces.length === 0" class="text-secondary">
      No workspaces yet. <!-- TODO: EmptyState component + create-CTA. -->
    </div>
    <table v-else class="w-full text-sm">
      <thead>
        <tr class="text-left">
          <th>Slug</th>
          <th>Name</th>
          <th>Plan</th>
          <th>Custom domain</th>
          <th>Status</th>
          <th>Members</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="ws in workspaces" :key="ws.id">
          <td>{{ ws.slug }}</td>
          <td>{{ ws.name }}</td>
          <td>{{ ws.plan }}</td>
          <td>{{ ws.custom_domain ?? '—' }}</td>
          <td>{{ ws.archived_at ? 'Archived' : 'Active' }}</td>
          <td>
            <router-link
              :to="{ name: 'admin-workspace-members', params: { id: ws.id } }"
            >
              Manage
            </router-link>
          </td>
          <td>
            <!-- TODO (Cursor): rename/archive/restore/delete buttons +
                 modals. Hard-delete must require the operator to type
                 the slug into a confirmation field — see the user-purge
                 confirm pattern in UsersListView for the established
                 shape. -->
            <button :disabled="isSaving">…</button>
          </td>
        </tr>
      </tbody>
    </table>

    <div v-if="errorMessage" class="text-status-error mt-4">
      {{ errorMessage }}
    </div>
    <div v-if="successMessage" class="text-status-success mt-4">
      {{ successMessage }}
    </div>
  </div>
</template>
