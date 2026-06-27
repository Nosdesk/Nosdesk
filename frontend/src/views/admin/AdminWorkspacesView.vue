<script setup lang="ts">
/**
 * Admin workspace lifecycle UI (Phase 4 W1).
 *
 * Lists workspaces, supports create / rename / archive / restore /
 * hard-delete (typed slug confirmation). Data layer uses Pinia Colada
 * `useQuery` keyed on `includeArchived`; see ApiTokensView.vue for
 * the reference caching pattern.
 */
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BulkConfirmDialog from '@/components/common/BulkConfirmDialog.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import Modal from '@/components/Modal.vue';
import workspacesService from '@nosdesk/core/services/workspacesService';
import type { Workspace } from '@nosdesk/core/types/workspace';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
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
  workspacesQuery.error.value ? t('admin-workspaces-error-load') : '',
);

// Edition / workspace-limit gate. Multi-workspace is a licensed feature and
// the affordance is hidden (not just disabled) until a deployment is entitled
// to it, mirroring how the advanced TLS option is gated out of production.
//
// Entitled today = self-hosted with an Enterprise license, under the licensed
// cap. Deliberately NOT offered on:
//   - pooled / hosted base tier (one workspace per account for now)
//   - self-hosted Community (no license)
// Silo/Enterprise hosted tiers will opt in here once the control plane
// provisions them. Defaults hidden until the edition query resolves so the
// button never flashes on an ineligible deployment.
const editionQuery = useQuery({
  key: ['admin-edition'] as const,
  query: () => workspacesService.getEdition(),
});
const canCreateWorkspace = computed(() => {
  const e = editionQuery.data.value;
  if (!e) return false;
  return e.self_hosted && e.edition === 'enterprise' && e.can_create_workspace;
});
// Explain the absence on self-hosted (cap reached / no license). Hosted base
// tier just gets no button, no upsell note.
const workspaceCapNote = computed(() => {
  const e = editionQuery.data.value;
  if (!e || !e.self_hosted || canCreateWorkspace.value) return '';
  return t('admin-workspaces-community-cap', { max: e.max_workspaces });
});

const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const refreshList = () =>
  queryCache.invalidateQueries({ key: WORKSPACES_KEY.value });

function flashSuccess(message: string) {
  successMessage.value = message;
  setTimeout(() => {
    successMessage.value = '';
  }, 3000);
}

// ---- Modal state ---------------------------------------------------

const showCreateModal = ref(false);
const showRenameModal = ref(false);
const workspaceToArchive = ref<Workspace | null>(null);
const workspaceToDelete = ref<Workspace | null>(null);

const createForm = ref({ slug: '', name: '' });
const renameForm = ref({ name: '' });
const workspaceToRename = ref<Workspace | null>(null);

function openCreateModal() {
  createForm.value = { slug: '', name: '' };
  showCreateModal.value = true;
}

function openRenameModal(ws: Workspace) {
  workspaceToRename.value = ws;
  renameForm.value = { name: ws.name };
  showRenameModal.value = true;
}

function openArchiveConfirm(ws: Workspace) {
  workspaceToArchive.value = ws;
}

function openDeleteConfirm(ws: Workspace) {
  if (!ws.archived_at) return;
  workspaceToDelete.value = ws;
}

// ---- Mutations -----------------------------------------------------

async function createWorkspace(payload: { slug: string; name: string }) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    const created = await workspacesService.create(payload);
    showCreateModal.value = false;
    flashSuccess(t('admin-workspaces-created-success', { slug: created.slug }));
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = extractErrorMessage(e, t('admin-workspaces-error-create'));
  } finally {
    isSaving.value = false;
  }
}

async function archiveWorkspace(id: number) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.archive(id);
    workspaceToArchive.value = null;
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = extractErrorMessage(e, t('admin-workspaces-error-archive'));
  } finally {
    isSaving.value = false;
  }
}

async function restoreWorkspace(id: number) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.restore(id);
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = extractErrorMessage(e, t('admin-workspaces-error-restore'));
  } finally {
    isSaving.value = false;
  }
}

async function renameWorkspace(id: number, name: string) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.rename(id, { name });
    showRenameModal.value = false;
    workspaceToRename.value = null;
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = extractErrorMessage(e, t('admin-workspaces-error-rename'));
  } finally {
    isSaving.value = false;
  }
}

async function hardDeleteWorkspace(id: number, confirmSlug: string) {
  isSaving.value = true;
  errorMessage.value = '';
  try {
    await workspacesService.hardDelete(id, confirmSlug);
    workspaceToDelete.value = null;
    refreshList();
  } catch (e: unknown) {
    errorMessage.value = extractErrorMessage(e, t('admin-workspaces-error-delete'));
  } finally {
    isSaving.value = false;
  }
}

function submitCreate() {
  const slug = createForm.value.slug.trim();
  const name = createForm.value.name.trim();
  if (!slug) {
    errorMessage.value = t('admin-workspaces-error-slug-required');
    return;
  }
  if (!name) {
    errorMessage.value = t('admin-workspaces-error-name-required');
    return;
  }
  void createWorkspace({ slug, name });
}

function submitRename() {
  const ws = workspaceToRename.value;
  if (!ws) return;
  const name = renameForm.value.name.trim();
  if (!name) {
    errorMessage.value = t('admin-workspaces-error-name-required');
    return;
  }
  void renameWorkspace(ws.id, name);
}

function confirmArchive() {
  const ws = workspaceToArchive.value;
  if (!ws) return;
  void archiveWorkspace(ws.id);
}

function confirmHardDelete() {
  const ws = workspaceToDelete.value;
  if (!ws) return;
  void hardDeleteWorkspace(ws.id, ws.slug);
}

const deleteTypeToConfirmLabel = computed(() => {
  const slug = workspaceToDelete.value?.slug ?? '';
  return t('admin-workspaces-delete-type-label', { slug });
});
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <header class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">
            {{ $t('admin-workspaces-title') }}
          </h1>
          <p class="text-secondary text-sm sm:text-base mt-1">
            {{ $t('admin-workspaces-description') }}
          </p>
        </div>
        <Button
          v-if="canCreateWorkspace"
          size="sm"
          icon="add"
          class="self-start sm:self-auto shrink-0"
          @click="openCreateModal"
        >
          {{ $t('admin-workspaces-create') }}
        </Button>
      </header>

      <!-- Self-hosted Community is capped at one workspace; surface why the
           Create action is disabled, with the upgrade path. -->
      <div
        v-if="workspaceCapNote"
        class="rounded-lg border border-subtle bg-surface-alt px-4 py-3 text-sm text-secondary flex items-start gap-2"
      >
        <Icon name="info" size="sm" class="text-tertiary shrink-0 mt-0.5" />
        <span>{{ workspaceCapNote }}</span>
      </div>

      <div class="flex flex-wrap items-center gap-4">
        <Checkbox
          id="include-archived"
          v-model="includeArchived"
          size="sm"
          :label="$t('admin-workspaces-include-archived')"
        />
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
      <AlertMessage
        v-if="loadError && workspaces.length === 0"
        type="error"
        :message="loadError"
      />

      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-workspaces-loading')"
        class="flex flex-col gap-2"
      >
        <div
          v-for="n in 4"
          :key="n"
          class="bg-surface border border-default rounded-lg p-3 flex items-center gap-3"
        >
          <SkeletonBar class="h-3 w-24" />
          <SkeletonBar class="h-3 w-40 flex-1" />
          <SkeletonBar class="h-3 w-16" />
          <SkeletonBar class="h-3 w-20 ml-auto" />
        </div>
      </Skeleton>

      <template v-else>
        <EmptyState
          v-if="workspaces.length === 0"
          icon="folder"
          :title="$t('empty-workspaces-title')"
          :description="$t('empty-workspaces-description')"
          :action-label="canCreateWorkspace ? $t('admin-workspaces-create') : undefined"
          variant="card"
          @action="openCreateModal"
        />

        <div
          v-else
          class="bg-surface border border-default rounded-xl overflow-hidden"
        >
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead class="bg-surface-alt text-tertiary">
                <tr>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-slug') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-name') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-plan') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-domain') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-status') }}
                  </th>
                  <th class="text-left px-3 py-2 font-medium">
                    {{ $t('admin-workspaces-col-members') }}
                  </th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody class="divide-y divide-subtle">
                <tr
                  v-for="ws in workspaces"
                  :key="ws.id"
                  class="bg-surface"
                  :class="{ 'opacity-60': ws.archived_at }"
                >
                  <td class="px-3 py-2 font-mono text-xs text-secondary">
                    {{ ws.slug }}
                  </td>
                  <td class="px-3 py-2 text-primary">{{ ws.name }}</td>
                  <td class="px-3 py-2 text-secondary">{{ ws.plan }}</td>
                  <td class="px-3 py-2 text-secondary">
                    {{ ws.custom_domain ?? $t('admin-workspaces-domain-none') }}
                  </td>
                  <td class="px-3 py-2">
                    <span
                      class="text-[10px] uppercase tracking-wide font-semibold rounded px-1.5 py-0.5"
                      :class="
                        ws.archived_at
                          ? 'text-secondary border border-default bg-surface-alt'
                          : 'text-status-success border border-status-success/30 bg-status-success/10'
                      "
                    >
                      {{
                        ws.archived_at
                          ? $t('admin-workspaces-status-archived')
                          : $t('admin-workspaces-status-active')
                      }}
                    </span>
                  </td>
                  <td class="px-3 py-2">
                    <router-link
                      :to="{ name: 'admin-workspace-members', params: { id: ws.id } }"
                      class="text-sm text-accent hover:underline"
                    >
                      {{ $t('admin-workspaces-members-link') }}
                    </router-link>
                  </td>
                  <td class="px-3 py-2">
                    <div class="flex items-center justify-end gap-1">
                      <button
                        type="button"
                        class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                        :aria-label="$t('admin-workspaces-action-rename')"
                        :disabled="isSaving"
                        @click="openRenameModal(ws)"
                      >
                        <Icon name="rename" class="w-3.5 h-3.5" />
                      </button>
                      <button
                        v-if="!ws.archived_at"
                        type="button"
                        class="p-1.5 text-secondary hover:text-status-warning hover:bg-status-warning/10 rounded-md transition-colors"
                        :aria-label="$t('admin-workspaces-archive')"
                        :disabled="isSaving"
                        @click="openArchiveConfirm(ws)"
                      >
                        <Icon name="archive" class="w-3.5 h-3.5" />
                      </button>
                      <button
                        v-else
                        type="button"
                        class="p-1.5 text-secondary hover:text-accent hover:bg-accent/10 rounded-md transition-colors"
                        :aria-label="$t('admin-workspaces-restore')"
                        :disabled="isSaving"
                        @click="restoreWorkspace(ws.id)"
                      >
                        <Icon name="restore" class="w-3.5 h-3.5" />
                      </button>
                      <button
                        type="button"
                        class="p-1.5 rounded-md transition-colors"
                        :class="
                          ws.archived_at
                            ? 'text-secondary hover:text-status-error hover:bg-status-error/10'
                            : 'text-tertiary cursor-not-allowed'
                        "
                        :aria-label="$t('admin-workspaces-delete')"
                        :title="
                          ws.archived_at
                            ? $t('admin-workspaces-delete')
                            : $t('admin-workspaces-delete-not-archived-hint')
                        "
                        :disabled="isSaving || !ws.archived_at"
                        @click="openDeleteConfirm(ws)"
                      >
                        <Icon name="trash" class="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </template>
    </div>

    <!-- Create workspace -->
    <Modal
      :show="showCreateModal"
      :title="$t('admin-workspaces-modal-create-title')"
      size="sm"
      @close="showCreateModal = false"
    >
      <form class="flex flex-col gap-4" @submit.prevent="submitCreate">
        <div>
          <label class="block text-sm font-medium text-primary mb-1">
            {{ $t('admin-workspaces-field-slug') }}
          </label>
          <input
            v-model="createForm.slug"
            type="text"
            autocomplete="off"
            spellcheck="false"
            :placeholder="$t('admin-workspaces-field-slug-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary font-mono text-sm placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
          <p class="text-xs text-tertiary mt-1">
            {{ $t('admin-workspaces-field-slug-hint') }}
          </p>
        </div>
        <div>
          <label class="block text-sm font-medium text-primary mb-1">
            {{ $t('admin-workspaces-field-name') }}
          </label>
          <input
            v-model="createForm.name"
            type="text"
            :placeholder="$t('admin-workspaces-field-name-placeholder')"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>
        <div class="flex justify-end gap-2 pt-2">
          <Button variant="ghost" type="button" @click="showCreateModal = false">
            {{ $t('admin-workspaces-cancel') }}
          </Button>
          <Button type="submit" :loading="isSaving">
            {{ $t('admin-workspaces-create') }}
          </Button>
        </div>
      </form>
    </Modal>

    <!-- Rename workspace (name only; slug is immutable) -->
    <Modal
      :show="showRenameModal"
      :title="$t('admin-workspaces-modal-rename-title')"
      size="sm"
      @close="showRenameModal = false"
    >
      <form class="flex flex-col gap-4" @submit.prevent="submitRename">
        <p v-if="workspaceToRename" class="text-sm text-secondary">
          {{ $t('admin-workspaces-rename-slug-note', { slug: workspaceToRename.slug }) }}
        </p>
        <div>
          <label class="block text-sm font-medium text-primary mb-1">
            {{ $t('admin-workspaces-field-name') }}
          </label>
          <input
            v-model="renameForm.name"
            type="text"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            required
          />
        </div>
        <div class="flex justify-end gap-2 pt-2">
          <Button variant="ghost" type="button" @click="showRenameModal = false">
            {{ $t('admin-workspaces-cancel') }}
          </Button>
          <Button type="submit" :loading="isSaving">
            {{ $t('admin-workspaces-rename-submit') }}
          </Button>
        </div>
      </form>
    </Modal>

    <!-- Archive confirmation -->
    <ConfirmModal
      :show="workspaceToArchive !== null"
      variant="warning"
      :title="$t('admin-workspaces-archive-confirm-title')"
      :message="
        workspaceToArchive
          ? t('admin-workspaces-archive-confirm-message', { name: workspaceToArchive.name })
          : ''
      "
      :confirm-label="$t('admin-workspaces-archive-confirm-label')"
      :loading="isSaving"
      @confirm="confirmArchive"
      @close="workspaceToArchive = null"
    />

    <!-- Hard delete: type the slug -->
    <BulkConfirmDialog
      :show="workspaceToDelete !== null"
      :title="$t('admin-workspaces-delete-title')"
      :message="
        workspaceToDelete
          ? t('admin-workspaces-delete-message', { name: workspaceToDelete.name })
          : ''
      "
      :confirm-label="$t('admin-workspaces-delete-confirm')"
      :require-confirm-text="workspaceToDelete?.slug"
      :type-to-confirm-label="deleteTypeToConfirmLabel"
      @confirm="confirmHardDelete"
      @close="workspaceToDelete = null"
    />
  </div>
</template>
