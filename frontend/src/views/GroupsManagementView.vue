<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BackButton from '@/components/common/BackButton.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { groupService } from '@/services/groupService';
import { useDataStore } from '@/stores/dataStore';
import type { GroupWithMemberCount, GroupWithMembers, CreateGroupRequest, UpdateGroupRequest } from '@/types/group';
import type { User } from '@/types/user';

const dataStore = useDataStore();

// State
const isLoading = ref(false);
const isSaving = ref(false);
const errorMessage = ref('');
const successMessage = ref('');
const groups = ref<GroupWithMemberCount[]>([]);

// Modal states
const showGroupModal = ref(false);
const showMembersModal = ref(false);
const showDeleteConfirm = ref(false);
const editingGroup = ref<GroupWithMembers | null>(null);
const selectedGroupForMembers = ref<GroupWithMembers | null>(null);
const groupToDelete = ref<GroupWithMemberCount | null>(null);

// Form state
const groupForm = ref<CreateGroupRequest>({
  name: '',
  description: '',
  color: '#6366f1'
});

// Available users for member selection
const availableUsers = ref<User[]>([]);
const selectedMemberUuids = ref<string[]>([]);
const userSearchQuery = ref('');

// Predefined colors
const colorOptions = [
  '#6366f1', '#8b5cf6', '#ec4899', '#ef4444', '#f97316',
  '#eab308', '#22c55e', '#14b8a6', '#06b6d4', '#3b82f6'
];

// Filtered users based on search
const filteredUsers = computed(() => {
  if (!userSearchQuery.value) return availableUsers.value;
  const query = userSearchQuery.value.toLowerCase();
  return availableUsers.value.filter(u =>
    u.name.toLowerCase().includes(query) ||
    (u.email && u.email.toLowerCase().includes(query))
  );
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

// Load available users
const loadUsers = async () => {
  try {
    const response = await dataStore.getPaginatedUsers({ page: 1, pageSize: 1000 });
    availableUsers.value = response.data;
  } catch (error) {
    console.error('Failed to load users:', error);
  }
};

// Open create group modal
const openCreateModal = () => {
  editingGroup.value = null;
  groupForm.value = {
    name: '',
    description: '',
    color: '#6366f1'
  };
  showGroupModal.value = true;
};

// Open edit group modal
const openEditModal = async (group: GroupWithMemberCount) => {
  try {
    const fullGroup = await groupService.getGroup(group.id);
    editingGroup.value = fullGroup;
    groupForm.value = {
      name: fullGroup.name,
      description: fullGroup.description || '',
      color: fullGroup.color || '#6366f1'
    };
    showGroupModal.value = true;
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load group details';
  }
};

// Open members modal
const openMembersModal = async (group: GroupWithMemberCount) => {
  try {
    const fullGroup = await groupService.getGroup(group.id);
    selectedGroupForMembers.value = fullGroup;
    selectedMemberUuids.value = fullGroup.members.map(m => m.uuid);
    userSearchQuery.value = '';
    await loadUsers();
    showMembersModal.value = true;
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load group members';
  }
};

// Toggle member selection
const toggleMember = (userUuid: string) => {
  const index = selectedMemberUuids.value.indexOf(userUuid);
  if (index === -1) {
    selectedMemberUuids.value.push(userUuid);
  } else {
    selectedMemberUuids.value.splice(index, 1);
  }
};

// Save group (create or update)
const saveGroup = async () => {
  if (!groupForm.value.name.trim()) {
    errorMessage.value = 'Group name is required';
    return;
  }

  isSaving.value = true;
  errorMessage.value = '';

  try {
    if (editingGroup.value) {
      const updateData: UpdateGroupRequest = {
        name: groupForm.value.name,
        description: groupForm.value.description || undefined,
        color: groupForm.value.color
      };
      await groupService.updateGroup(editingGroup.value.id, updateData);
      successMessage.value = 'Group updated successfully';
    } else {
      await groupService.createGroup(groupForm.value);
      successMessage.value = 'Group created successfully';
    }

    showGroupModal.value = false;
    await loadGroups();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to save group';
  } finally {
    isSaving.value = false;
  }
};

// Save members
const saveMembers = async () => {
  if (!selectedGroupForMembers.value) return;

  isSaving.value = true;
  errorMessage.value = '';

  try {
    await groupService.setGroupMembers(selectedGroupForMembers.value.id, {
      member_uuids: selectedMemberUuids.value
    });

    successMessage.value = 'Members updated successfully';
    showMembersModal.value = false;
    await loadGroups();

    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update members';
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
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:opacity-90 font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        New Group
      </button>
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-2xl font-bold text-primary">Groups</h1>
        <p class="text-secondary mt-1">Manage user groups and memberships</p>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading groups..." />

      <!-- Groups list -->
      <div v-else class="flex flex-col gap-3">
        <div
          v-for="group in groups"
          :key="group.id"
          class="bg-surface border border-default rounded-xl hover:border-strong transition-colors"
        >
          <div class="p-4 flex items-center gap-4">
            <!-- Color indicator -->
            <div
              class="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0"
              :style="{ backgroundColor: (group.color || '#6366f1') + '20' }"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="h-5 w-5"
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
              <div class="flex items-center gap-2">
                <h3 class="font-medium text-primary">{{ group.name }}</h3>
                <span class="px-2 py-0.5 text-xs bg-surface-alt text-secondary rounded-full">
                  {{ group.member_count }} member{{ group.member_count !== 1 ? 's' : '' }}
                </span>
              </div>
              <p v-if="group.description" class="text-sm text-secondary mt-0.5 truncate">
                {{ group.description }}
              </p>
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-2 flex-shrink-0">
              <button
                @click="openMembersModal(group)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                title="Manage members"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z" />
                </svg>
              </button>
              <button
                @click="openEditModal(group)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                title="Edit group"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              <button
                @click="confirmDelete(group)"
                class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
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

    <!-- Create/Edit Group Modal -->
    <Modal
      :show="showGroupModal"
      :title="editingGroup ? 'Edit Group' : 'Create Group'"
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
          <label class="block text-sm font-medium text-primary mb-2">Color</label>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="color in colorOptions"
              :key="color"
              type="button"
              @click="groupForm.color = color"
              class="w-8 h-8 rounded-lg transition-transform hover:scale-110"
              :class="{ 'ring-2 ring-offset-2 ring-accent ring-offset-surface': groupForm.color === color }"
              :style="{ backgroundColor: color }"
            />
          </div>
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
            {{ editingGroup ? 'Save Changes' : 'Create Group' }}
          </button>
        </div>
      </form>
    </Modal>

    <!-- Manage Members Modal -->
    <Modal
      :show="showMembersModal"
      :title="`Manage Members - ${selectedGroupForMembers?.name || ''}`"
      size="md"
      @close="showMembersModal = false"
    >
      <div class="flex flex-col gap-4">
        <!-- Search -->
        <div class="relative">
          <svg xmlns="http://www.w3.org/2000/svg" class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
          <input
            v-model="userSearchQuery"
            type="text"
            placeholder="Search users..."
            class="w-full pl-10 pr-4 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />
        </div>

        <!-- Selected count -->
        <div class="text-sm text-secondary">
          {{ selectedMemberUuids.length }} user{{ selectedMemberUuids.length !== 1 ? 's' : '' }} selected
        </div>

        <!-- User list -->
        <div class="max-h-80 overflow-y-auto border border-default rounded-lg divide-y divide-default">
          <label
            v-for="user in filteredUsers"
            :key="user.uuid"
            class="flex items-center gap-3 p-3 hover:bg-surface-hover cursor-pointer transition-colors"
          >
            <input
              type="checkbox"
              :checked="selectedMemberUuids.includes(user.uuid)"
              @change="toggleMember(user.uuid)"
              class="w-4 h-4 text-accent bg-surface-alt border-default rounded focus:ring-accent focus:ring-offset-0"
            />
            <UserAvatar
              :name="user.uuid"
              :userName="user.name"
              :avatar="user.avatar_thumb || user.avatar_url"
              size="sm"
              :clickable="false"
              :show-name="false"
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
              <div v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</div>
            </div>
          </label>

          <div v-if="filteredUsers.length === 0" class="p-4 text-center text-tertiary">
            No users found
          </div>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            @click="showMembersModal = false"
            class="px-4 py-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
          >
            Cancel
          </button>
          <button
            @click="saveMembers"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            <svg v-if="isSaving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            Save Members
          </button>
        </div>
      </div>
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
