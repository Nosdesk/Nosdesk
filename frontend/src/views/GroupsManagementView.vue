<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useRouter } from 'vue-router';

import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
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
import type { GroupWithMemberCount, CreateGroupRequest } from '@/types/group';

const router = useRouter();
const { colorFilterStyle } = useColorFilter();

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

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const groups = ref<GroupWithMemberCount[]>([]);

// Search & sort
const searchQuery = ref('');
const sortField = ref('name');
const sortAsc = ref(true);

const sortFieldOptions = [
  { value: 'name', label: 'Name' },
  { value: 'members', label: 'Members' },
  { value: 'devices', label: 'Devices' },
  { value: 'created', label: 'Date Added' },
];

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

// Load groups
const loadGroups = async () => {
  isLoading.value = true;
  errorMessage.value = '';

  try {
    const result = await groupService.getGroups();
    if (Array.isArray(result)) {
      groups.value = result;
    } else {
      console.error('Unexpected groups response:', result);
      groups.value = [];
    }
  } catch (error) {
    console.error('Failed to load groups:', error);
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load groups';
    groups.value = [];
  } finally {
    isLoading.value = false;
  }
};

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
    errorMessage.value = 'Group name is required';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await groupService.createGroup(groupForm.value);
    successMessage.value = 'Group created successfully';

    showGroupModal.value = false;
    await loadGroups();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to create group';
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
    successMessage.value = 'Group deleted successfully';
    showDeleteConfirm.value = false;

    // Clear panel if the deleted group was selected
    if (selectedGroupUuid.value === groupToDelete.value.uuid) {
      selectedGroupUuid.value = null;
    }

    groupToDelete.value = null;
    await loadGroups();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to delete group';
  } finally {
    isSaving.value = false;
  }
};

// Panel event handlers
const onPanelDeleted = () => {
  selectedGroupUuid.value = null;
  loadGroups();
};

const onPanelUpdated = () => {
  loadGroups();
};

const onPanelClose = () => {
  selectedGroupUuid.value = null;
};

onMounted(() => {
  loadGroups();
});
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
            <h1 class="text-xl sm:text-2xl font-bold text-primary">Groups</h1>
            <p class="text-secondary text-sm sm:text-base mt-1">Manage user groups and memberships</p>
          </div>
          <button
            @click="openCreateModal"
            class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5 self-start sm:self-auto"
          >
            <Icon name="add" />
            <span class="hidden xs:inline">New Group</span>
            <span class="xs:hidden">New</span>
          </button>
        </div>

        <!-- Success message -->
        <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

        <!-- Error message -->
        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <!-- Loading state -->
        <LoadingSpinner v-if="isLoading" text="Loading groups..." />

        <!-- Search & sort toolbar -->
        <div v-if="!isLoading && groups.length > 0" class="flex items-center gap-2">
          <DebouncedSearchInput
            v-model="searchQuery"
            placeholder="Search groups..."
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
            :title="sortAsc ? 'Ascending' : 'Descending'"
          >
            <Icon
              name="chevronUp"
              class="transition-transform duration-200"
              :class="{ 'rotate-180': !sortAsc }"
            />
          </button>
        </div>

        <!-- Groups list -->
        <div v-if="!isLoading" class="flex flex-col gap-2 sm:gap-3">
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
                      {{ group.member_count }} member{{ group.member_count !== 1 ? 's' : '' }}
                    </span>
                    <span v-if="group.device_count > 0" class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ group.device_count }} device{{ group.device_count !== 1 ? 's' : '' }}
                    </span>
                    <span v-if="group.included_group_count > 0" class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ group.included_group_count }} group{{ group.included_group_count !== 1 ? 's' : '' }}
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
                  title="Open full page"
                >
                  <Icon name="settings" />
                </button>
                <button
                  @click.stop="confirmDelete(group)"
                  class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                  title="Delete group"
                >
                  <Icon name="trash" />
                </button>
              </div>
            </div>
          </div>

          <!-- No search results -->
          <div v-if="filteredGroups.length === 0 && groups.length > 0" class="text-center py-8 text-secondary text-sm">
            No groups matching "{{ searchQuery }}"
          </div>

          <!-- Empty state -->
          <EmptyState
            v-if="groups.length === 0 && !isLoading"
            icon="users"
            :title="$t('empty-groups-title')"
            :description="$t('empty-groups-description')"
            action-label="Create Group"
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
    title="Create Group"
    size="sm"
    @close="showGroupModal = false"
  >
    <form @submit.prevent="saveGroup" class="flex flex-col gap-4">
      <!-- Name -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">Name</label>
        <input
          v-model="groupForm.name"
          type="text"
          placeholder="Enter group name"
          class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          required
        />
      </div>

      <!-- Description -->
      <div>
        <label class="block text-sm font-medium text-primary mb-1">Description</label>
        <textarea
          v-model="groupForm.description"
          placeholder="Optional description"
          rows="2"
          class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
        />
      </div>

      <!-- Color -->
      <div>
        <ColorHueSlider
          :model-value="groupForm.color ?? '#6366f1'"
          @update:model-value="groupForm.color = $event"
          label="Color"
        />
      </div>

      <!-- Actions -->
      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          @click="showGroupModal = false"
          class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
        >
          Cancel
        </button>
        <button
          type="submit"
          :disabled="isSaving"
          class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Spinner v-if="isSaving" />
          Create Group
        </button>
      </div>
    </form>
  </Modal>

  <!-- Delete Confirmation Modal -->
  <Modal
    :show="showDeleteConfirm"
    title="Delete Group"
    size="sm"
    @close="showDeleteConfirm = false"
  >
    <div class="flex flex-col gap-4">
      <p class="text-secondary">
        Are you sure you want to delete the group <strong class="text-primary">{{ groupToDelete?.name }}</strong>?
        This will remove all member associations but will not delete the users.
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          @click="showDeleteConfirm = false"
          class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
        >
          Cancel
        </button>
        <button
          @click="deleteGroup"
          :disabled="isSaving"
          class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
        >
          <Spinner v-if="isSaving" />
          Delete Group
        </button>
      </div>
    </div>
  </Modal>
  </div>
</template>
