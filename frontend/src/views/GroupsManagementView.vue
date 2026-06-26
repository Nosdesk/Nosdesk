<script setup lang="ts">
import { ref, computed } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import DebouncedSearchInput from '@/components/common/DebouncedSearchInput.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import SplitPanelLayout from '@/components/admin/SplitPanelLayout.vue';
import GroupConfigurationPanel from '@/components/admin/GroupConfigurationPanel.vue';
import { groupService } from '@/services/groupService';
import { useColorFilter } from '@/composables/useColorFilter';
import { GROUPS_QUERY_KEY } from '@/composables/useAssignmentPickerQueries';
import type { GroupWithMemberCount, CreateGroupRequest } from '@nosdesk/core/types/group';
import { extractErrorMessage } from '@/utils/errors';

const router = useRouter();
const { colorFilterStyle } = useColorFilter();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

// Selected group for split-panel
const selectedGroupUuid = ref<string | null>(null);

// Click item → open side panel (desktop) or navigate to configure page (mobile)
const selectGroup = (group: GroupWithMemberCount, isMobile: boolean) => {
  if (isMobile) {
    router.push(`/admin/groups/${group.uuid}/configure`);
  } else {
    selectedGroupUuid.value = group.uuid;
  }
};

// Cog icon → always navigate to the full configure page
const navigateToConfiguration = (group: GroupWithMemberCount) => {
  router.push(`/admin/groups/${group.uuid}/configure`);
};

// The group list is cached by Pinia Colada keyed here, so navigating
// away and back renders it instantly from cache and revalidates in the
// background. A skeleton shows only on the genuine first load (empty
// cache); see `isFirstLoad`.
const GROUPS_KEY = GROUPS_QUERY_KEY;
const queryCache = useQueryCache();
const groupsQuery = useQuery({
  key: GROUPS_KEY,
  query: () => groupService.getGroups(),
});
const groups = computed<GroupWithMemberCount[]>(() =>
  Array.isArray(groupsQuery.data.value) ? groupsQuery.data.value : (groupsQuery.data.value ?? []),
);
const isFirstLoad = computed(
  () => groupsQuery.status.value === 'pending' && groupsQuery.data.value === undefined,
);
const loadError = computed(() =>
  groupsQuery.error.value ? t('groups-mgmt-error-load') : '',
);

// Mutation feedback (create / delete) stays in local refs.
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

// Search & sort
const searchQuery = ref('');
const sortField = ref('name');
const sortAsc = ref(true);

const sortFieldOptions = computed(() => [
  { value: 'name', label: t('groups-mgmt-sort-name') },
  { value: 'members', label: t('groups-mgmt-sort-members') },
  { value: 'devices', label: t('groups-mgmt-sort-assets') },
  { value: 'created', label: t('groups-mgmt-sort-created') },
]);

// Sensible default directions per field
const fieldDefaultAsc: Record<string, boolean> = {
  name: true,       // A-Z
  members: false,   // Most first
  devices: false,   // Most first
  created: false,   // Newest first
};

const toggleSortDirection = () => {
  sortAsc.value = !sortAsc.value;
};

// When switching fields, apply the sensible default direction
const onSortFieldChange = (value: string | string[]) => {
  const field = Array.isArray(value) ? value[0] : value;
  sortField.value = field;
  sortAsc.value = fieldDefaultAsc[field] ?? true;
};

const filteredGroups = computed(() => {
  let result = [...groups.value];

  // Search filter
  if (searchQuery.value.trim()) {
    const q = searchQuery.value.toLowerCase();
    result = result.filter(g =>
      g.name.toLowerCase().includes(q) ||
      (g.description && g.description.toLowerCase().includes(q))
    );
  }

  // Sort
  const dir = sortAsc.value ? 1 : -1;
  result.sort((a, b) => {
    switch (sortField.value) {
      case 'name': return a.name.localeCompare(b.name) * dir;
      case 'members': return (a.member_count - b.member_count) * dir;
      case 'devices': return (a.device_count - b.device_count) * dir;
      case 'created': return (new Date(a.created_at).getTime() - new Date(b.created_at).getTime()) * dir;
      default: return 0;
    }
  });

  return result;
});

// Modal states
const showGroupModal = ref(false);
const showDeleteConfirm = ref(false);
const groupToDelete = ref<GroupWithMemberCount | null>(null);

// Form state
const groupForm = ref<CreateGroupRequest>({
  name: '',
  description: '',
  color: '#6366f1'
});

// Open create group modal
const openCreateModal = () => {
  groupForm.value = {
    name: '',
    description: '',
    color: '#6366f1'
  };
  showGroupModal.value = true;
};

// Save group (create)
const saveGroup = async () => {
  if (!groupForm.value.name.trim()) {
    errorMessage.value = t('groups-mgmt-error-name-required');
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await groupService.createGroup(groupForm.value);
    successMessage.value = t('groups-mgmt-success-created');

    showGroupModal.value = false;
    await queryCache.invalidateQueries({ key: GROUPS_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('groups-mgmt-error-create'));
  } finally {
    isSaving.value = false;
  }
};

// Confirm delete
const confirmDelete = (group: GroupWithMemberCount) => {
  groupToDelete.value = group;
  showDeleteConfirm.value = true;
};

// Delete group
const deleteGroup = async () => {
  if (!groupToDelete.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await groupService.deleteGroup(groupToDelete.value.id);
    successMessage.value = t('groups-mgmt-success-deleted');
    showDeleteConfirm.value = false;

    // Clear panel if the deleted group was selected
    if (selectedGroupUuid.value === groupToDelete.value.uuid) {
      selectedGroupUuid.value = null;
    }

    groupToDelete.value = null;
    await queryCache.invalidateQueries({ key: GROUPS_KEY });

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    errorMessage.value = extractErrorMessage(error, t('groups-mgmt-error-delete'));
  } finally {
    isSaving.value = false;
  }
};

// Panel event handlers
const onPanelDeleted = () => {
  selectedGroupUuid.value = null;
  queryCache.invalidateQueries({ key: GROUPS_KEY });
};

const onPanelUpdated = () => {
  queryCache.invalidateQueries({ key: GROUPS_KEY });
};

const onPanelClose = () => {
  selectedGroupUuid.value = null;
};
</script>

<template>
  <!-- Single root so App.vue's <Transition mode="out-in"> can attach
       leave/enter classes. Modals stay inside the wrapper; they
       teleport themselves at render time. -->
  <div class="h-full">
  <SplitPanelLayout :panelOpen="!!selectedGroupUuid">
    <template #list="{ isMobile }">
      <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full" :class="selectedGroupUuid && !isMobile ? '' : 'max-w-8xl'">
        <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
          <div>
            <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('groups-mgmt-title') }}</h1>
            <p class="text-secondary text-sm sm:text-base mt-1">{{ $t('groups-mgmt-subtitle') }}</p>
          </div>
          <button
            @click="openCreateModal"
            class="px-3 py-1.5 bg-accent text-on-accent rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5 self-start sm:self-auto"
          >
            <Icon name="add" />
            <span class="hidden xs:inline">{{ $t('groups-mgmt-action-new') }}</span>
            <span class="xs:hidden">{{ $t('groups-mgmt-action-new-short') }}</span>
          </button>
        </div>

        <!-- Success message -->
        <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

        <!-- Error message -->
        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <!-- Load error (initial fetch failed with no cached data) -->
        <AlertMessage v-if="loadError && groups.length === 0" type="error" :message="loadError" />

        <!-- First-load skeleton: mirrors the group-row layout so the
             shell doesn't shift when data arrives. Only shown on a cold
             cache; remounts render cached rows instantly and revalidate
             silently in the background. -->
        <Skeleton
          v-if="isFirstLoad"
          :label="$t('groups-mgmt-loading')"
          class="flex flex-col gap-2 sm:gap-3"
        >
          <div
            v-for="n in 3"
            :key="n"
            class="bg-surface border border-default rounded-lg sm:rounded-xl p-3 sm:p-4 flex items-center gap-3 sm:gap-4"
          >
            <SkeletonBar class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg shrink-0" />
            <div class="flex-1 flex flex-col gap-2">
              <SkeletonBar class="h-4 w-40 max-w-full" />
              <SkeletonBar class="h-3 w-3/4" />
            </div>
          </div>
        </Skeleton>

        <!-- Search & sort toolbar -->
        <div v-if="!isFirstLoad && groups.length > 0" class="flex items-center gap-2">
          <DebouncedSearchInput
            v-model="searchQuery"
            :placeholder="$t('groups-mgmt-search-placeholder')"
            :debounce-ms="0"
            class="max-w-xs"
          />
          <BaseDropdown
            :model-value="sortField"
            :options="sortFieldOptions"
            size="sm"
            @update:model-value="onSortFieldChange"
          />
          <button
            @click="toggleSortDirection"
            class="p-1.5 border border-default rounded-lg bg-surface-alt hover:border-strong hover:bg-surface-hover transition-colors text-secondary hover:text-primary"
            :title="sortAsc ? $t('groups-mgmt-sort-ascending') : $t('groups-mgmt-sort-descending')"
          >
            <Icon
              name="chevronUp"
              class="transition-transform duration-200"
              :class="{ 'rotate-180': !sortAsc }"
            />
          </button>
        </div>

        <!-- Groups list -->
        <div v-if="!isFirstLoad" class="flex flex-col gap-2 sm:gap-3">
          <div
            v-for="group in filteredGroups"
            :key="group.id"
            class="bg-surface border rounded-lg sm:rounded-xl transition-colors cursor-pointer"
            :class="selectedGroupUuid === group.uuid && !isMobile
              ? 'border-accent bg-accent/5'
              : 'border-default hover:border-strong'"
            @click="selectGroup(group, isMobile)"
          >
            <div class="p-3 sm:p-4 flex items-center gap-3 sm:gap-4">
              <!-- Color indicator -->
              <div
                class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg flex items-center justify-center flex-shrink-0"
                :style="{ backgroundColor: (group.color || '#6366f1') + '20', ...colorFilterStyle }"
              >
                <Icon
                  name="team"
                  class="sm:h-5 sm:w-5"
                  :style="{ color: group.color || '#6366f1' }"
                />
              </div>

              <!-- Group info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-primary text-sm sm:text-base truncate">{{ group.name }}</h3>
                  <div class="flex items-center gap-1.5">
                    <span class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ $t('groups-mgmt-chip-members', { count: group.member_count }) }}
                    </span>
                    <span v-if="group.device_count > 0" class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ $t('groups-mgmt-chip-devices', { count: group.device_count }) }}
                    </span>
                    <span v-if="group.included_group_count > 0" class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ $t('groups-mgmt-chip-groups', { count: group.included_group_count }) }}
                    </span>
                  </div>
                </div>
                <p v-if="group.description" class="text-xs sm:text-sm text-secondary mt-0.5 truncate">
                  {{ group.description }}
                </p>
              </div>

              <!-- Actions -->
              <div class="flex items-center gap-0.5 sm:gap-1 flex-shrink-0">
                <button
                  @click.stop="navigateToConfiguration(group)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                  :title="$t('groups-mgmt-action-open-full-page')"
                >
                  <Icon name="settings" />
                </button>
                <button
                  @click.stop="confirmDelete(group)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  :title="$t('groups-mgmt-action-delete')"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>

          <!-- No search results -->
          <div v-if="filteredGroups.length === 0 && groups.length > 0" class="text-center py-8 text-secondary text-sm">
            {{ $t('groups-mgmt-no-results', { query: searchQuery }) }}
          </div>

          <!-- Empty state -->
          <EmptyState
            v-if="groups.length === 0 && !isFirstLoad"
            icon="users"
            :title="$t('empty-groups-title')"
            :description="$t('empty-groups-description')"
            :action-label="$t('groups-mgmt-empty-action')"
            variant="card"
            @action="openCreateModal"
          />
        </div>
      </div>
    </template>

    <template #panel>
      <GroupConfigurationPanel
        v-if="selectedGroupUuid"
        :groupUuid="selectedGroupUuid"
        class="flex-1"
        @close="onPanelClose"
        @deleted="onPanelDeleted"
        @updated="onPanelUpdated"
      />
    </template>
  </SplitPanelLayout>

  <!-- Create Group Modal -->
  <Modal
    :show="showGroupModal"
    :title="$t('groups-mgmt-modal-create-title')"
    size="sm"
    @close="showGroupModal = false"
  >
    <form @submit.prevent="saveGroup" class="flex flex-col gap-4">
      <!-- Name -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">{{ $t('groups-mgmt-field-name') }}</label>
        <input
          v-model="groupForm.name"
          type="text"
          :placeholder="$t('groups-mgmt-field-name-placeholder')"
          class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          required
        />
      </div>

      <!-- Description -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">{{ $t('groups-mgmt-field-description') }}</label>
        <textarea
          v-model="groupForm.description"
          :placeholder="$t('groups-mgmt-field-description-placeholder')"
          rows="2"
          class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
        />
      </div>

      <!-- Color -->
      <div>
        <ColorHueSlider
          :model-value="groupForm.color ?? '#6366f1'"
          @update:model-value="groupForm.color = $event"
          :label="$t('groups-mgmt-field-color')"
        />
      </div>

      <!-- Actions -->
      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          @click="showGroupModal = false"
          class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
        >
          {{ $t('groups-mgmt-action-cancel') }}
        </button>
        <button
          type="submit"
          :disabled="isSaving"
          class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Spinner v-if="isSaving" />
          {{ $t('groups-mgmt-action-create') }}
        </button>
      </div>
    </form>
  </Modal>

  <!-- Delete Confirmation Modal -->
  <Modal
    :show="showDeleteConfirm"
    :title="$t('groups-mgmt-modal-delete-title')"
    size="sm"
    @close="showDeleteConfirm = false"
  >
    <div class="flex flex-col gap-4">
      <p class="text-secondary" v-html="$t('groups-mgmt-delete-confirm-body', { name: groupToDelete?.name ?? '' })"></p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          @click="showDeleteConfirm = false"
          class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
        >
          {{ $t('groups-mgmt-action-cancel') }}
        </button>
        <button
          @click="deleteGroup"
          :disabled="isSaving"
          class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Spinner v-if="isSaving" />
          {{ $t('groups-mgmt-action-delete-confirm') }}
        </button>
      </div>
    </div>
  </Modal>
  </div>
</template>
