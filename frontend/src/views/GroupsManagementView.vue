<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import { groupService } from '@/services/groupService';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupWithMemberCount, CreateGroupRequest } from '@/types/group';

const router = useRouter();
const { colorFilterStyle } = useColorFilter();

// Navigate to group detail page
const navigateToGroup = (group: GroupWithMemberCount) => {
  router.push(`/groups/${group.uuid}`);
};

// Navigate to group configuration page
const navigateToConfiguration = (group: GroupWithMemberCount) => {
  router.push(`/admin/groups/${group.uuid}/configure`);
};

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const groups = ref<GroupWithMemberCount[]>([]);

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
    // Ensure we got an array
    if (Array.isArray(result)) {
      groups.value = result;
      console.log('Loaded groups:', result.length);
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

onMounted(() => {
  loadGroups();
});
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:bg-accent-hover font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        <span class="hidden xs:inline">New Group</span>
        <span class="xs:hidden">New</span>
      </button>
    </div>

    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">Groups</h1>
        <p class="text-secondary text-sm sm:text-base mt-1">Manage user groups and memberships</p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading groups..." />

      <!-- Groups list -->
      <div v-else class="flex flex-col gap-2 sm:gap-3">
        <div
          v-for="group in groups"
          :key="group.id"
          class="bg-surface border border-default rounded-lg sm:rounded-xl hover:border-strong transition-colors"
        >
          <div class="p-3 sm:p-4 flex items-center gap-3 sm:gap-4">
            <!-- Clickable area for navigation -->
            <div
              @click="navigateToGroup(group)"
              class="flex items-center gap-3 sm:gap-4 flex-1 min-w-0 cursor-pointer group"
            >
              <!-- Color indicator -->
              <div
                class="w-8 h-8 sm:w-10 sm:h-10 rounded-lg flex items-center justify-center flex-shrink-0 transition-transform group-hover:scale-105"
                :style="{ backgroundColor: (group.color || '#6366f1') + '20', ...colorFilterStyle }"
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  class="h-4 w-4 sm:h-5 sm:w-5"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="2"
                  :style="{ color: group.color || '#6366f1' }"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                </svg>
              </div>

              <!-- Group info -->
              <div class="flex-1 min-w-0">
                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-2">
                  <h3 class="font-medium text-primary text-sm sm:text-base group-hover:text-accent transition-colors truncate">{{ group.name }}</h3>
                  <div class="flex items-center gap-1.5">
                    <span class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ group.member_count }} member{{ group.member_count !== 1 ? 's' : '' }}
                    </span>
                    <span v-if="group.device_count > 0" class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                      {{ group.device_count }} device{{ group.device_count !== 1 ? 's' : '' }}
                    </span>
                  </div>
                </div>
                <p v-if="group.description" class="text-xs sm:text-sm text-secondary mt-0.5 truncate">
                  {{ group.description }}
                </p>
              </div>
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-0.5 sm:gap-1 flex-shrink-0">
              <button
                @click.stop="navigateToConfiguration(group)"
                class="p-1.5 sm:p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-md sm:rounded-lg transition-colors"
                title="Configure group"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                </svg>
              </button>
              <button
                @click.stop="confirmDelete(group)"
                class="p-1.5 sm:p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-md sm:rounded-lg transition-colors"
                title="Delete group"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="groups.length === 0 && !isLoading"
          icon="users"
          title="No groups yet"
          description="Create your first group to organize users"
          action-label="Create Group"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

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
          <ColorHueSlider v-model="groupForm.color" label="Color" />
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
            <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
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
            <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Delete Group
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
