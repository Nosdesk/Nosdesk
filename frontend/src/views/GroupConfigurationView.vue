<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { groupService } from '@/services/groupService';
import { useDataStore } from '@/stores/dataStore';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupDetails, UpdateGroupRequest } from '@/types/group';
import type { User } from '@/types/user';

const route = useRoute();
const router = useRouter();
const dataStore = useDataStore();
const { colorFilterStyle } = useColorFilter();

// State
const group = ref<GroupDetails | null>(null);
const loading = ref(true);
const saving = ref(false);
const savingMembers = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

// Form state
const generalForm = ref({
  name: '',
  description: '',
  color: '#6366f1'
});

// Members state
const availableUsers = ref<User[]>([]);
const selectedMemberUuids = ref<string[]>([]);
const userSearchQuery = ref('');

// Delete confirmation
const showDeleteConfirm = ref(false);
const isDeleting = ref(false);

// Filtered users based on search
const filteredUsers = computed(() => {
  if (!userSearchQuery.value) return availableUsers.value;
  const query = userSearchQuery.value.toLowerCase();
  return availableUsers.value.filter(u =>
    u.name.toLowerCase().includes(query) ||
    (u.email && u.email.toLowerCase().includes(query))
  );
});

// Check if general form has changes
const hasGeneralChanges = computed(() => {
  if (!group.value) return false;
  return (
    generalForm.value.name !== group.value.name ||
    generalForm.value.description !== (group.value.description || '') ||
    generalForm.value.color !== (group.value.color || '#6366f1')
  );
});

// Check if members have changes
const hasMemberChanges = computed(() => {
  if (!group.value) return false;
  const currentUuids = group.value.members.map(m => m.uuid).sort();
  const selectedUuids = [...selectedMemberUuids.value].sort();
  return JSON.stringify(currentUuids) !== JSON.stringify(selectedUuids);
});

// Load group data
const loadGroup = async () => {
  try {
    loading.value = true;
    errorMessage.value = '';

    const uuid = route.params.uuid as string;
    if (!uuid) {
      errorMessage.value = 'Invalid group ID';
      loading.value = false;
      return;
    }

    group.value = await groupService.getGroupDetails(uuid);

    // Populate form
    generalForm.value = {
      name: group.value.name,
      description: group.value.description || '',
      color: group.value.color || '#6366f1'
    };

    // Populate selected members
    selectedMemberUuids.value = group.value.members.map(m => m.uuid);
  } catch (e) {
    errorMessage.value = 'Failed to load group details';
    console.error('Error loading group:', e);
  } finally {
    loading.value = false;
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

// Save general info
const saveGeneralInfo = async () => {
  if (!group.value || !generalForm.value.name.trim()) {
    errorMessage.value = 'Group name is required';
    return;
  }

  saving.value = true;
  errorMessage.value = '';

  try {
    const updateData: UpdateGroupRequest = {
      name: generalForm.value.name,
      description: generalForm.value.description || undefined,
      color: generalForm.value.color
    };

    await groupService.updateGroup(group.value.id, updateData);

    // Update local state
    group.value.name = generalForm.value.name;
    group.value.description = generalForm.value.description || null;
    group.value.color = generalForm.value.color;

    successMessage.value = 'Group updated successfully';
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to save group';
  } finally {
    saving.value = false;
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

// Save members
const saveMembers = async () => {
  if (!group.value) return;

  savingMembers.value = true;
  errorMessage.value = '';

  try {
    await groupService.setGroupMembers(group.value.id, {
      member_uuids: selectedMemberUuids.value
    });

    // Reload group to get updated member list
    await loadGroup();

    successMessage.value = 'Members updated successfully';
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update members';
  } finally {
    savingMembers.value = false;
  }
};

// Delete group
const deleteGroup = async () => {
  if (!group.value) return;

  isDeleting.value = true;
  errorMessage.value = '';

  try {
    await groupService.deleteGroup(group.value.id);
    router.push('/admin/groups');
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to delete group';
    showDeleteConfirm.value = false;
  } finally {
    isDeleting.value = false;
  }
};

onMounted(() => {
  loadGroup();
  loadUsers();
});
</script>

<template>
  <div class="flex-1">
    <!-- Loading State with Skeleton -->
    <div v-if="loading" class="flex flex-col">
      <!-- Skeleton Navigation bar -->
      <div class="pt-4 px-4 sm:px-6 flex items-center justify-between">
        <div class="h-8 w-32 bg-surface-alt rounded-lg animate-pulse"></div>
        <div class="h-8 w-24 bg-surface-alt rounded-lg animate-pulse"></div>
      </div>

      <!-- Skeleton Main Content -->
      <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
        <!-- Skeleton Header -->
        <div class="flex flex-col gap-2">
          <div class="h-7 w-56 bg-surface-alt rounded animate-pulse"></div>
          <div class="h-4 w-72 bg-surface-alt rounded animate-pulse"></div>
        </div>

        <!-- Skeleton Cards Grid -->
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 sm:gap-6">
          <!-- Skeleton Card 1 -->
          <div class="bg-surface border border-default rounded-xl overflow-hidden">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <div class="h-5 w-40 bg-surface rounded animate-pulse"></div>
            </div>
            <div class="p-4 space-y-4">
              <div class="space-y-2">
                <div class="h-4 w-16 bg-surface-alt rounded animate-pulse"></div>
                <div class="h-10 w-full bg-surface-alt rounded-lg animate-pulse"></div>
              </div>
              <div class="space-y-2">
                <div class="h-4 w-24 bg-surface-alt rounded animate-pulse"></div>
                <div class="h-20 w-full bg-surface-alt rounded-lg animate-pulse"></div>
              </div>
              <div class="space-y-2">
                <div class="h-4 w-12 bg-surface-alt rounded animate-pulse"></div>
                <div class="flex gap-2">
                  <div v-for="i in 5" :key="i" class="w-8 h-8 bg-surface-alt rounded-lg animate-pulse"></div>
                </div>
              </div>
            </div>
          </div>

          <!-- Skeleton Card 2 -->
          <div class="bg-surface border border-default rounded-xl overflow-hidden">
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <div class="h-5 w-24 bg-surface rounded animate-pulse"></div>
            </div>
            <div class="p-4 space-y-3">
              <div class="h-10 w-full bg-surface-alt rounded-lg animate-pulse"></div>
              <div v-for="i in 4" :key="i" class="flex items-center gap-3">
                <div class="w-5 h-5 bg-surface-alt rounded animate-pulse"></div>
                <div class="w-8 h-8 rounded-full bg-surface-alt animate-pulse"></div>
                <div class="h-4 w-32 bg-surface-alt rounded animate-pulse"></div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Error State -->
    <div v-else-if="errorMessage && !group" class="p-4 sm:p-6">
      <AlertMessage type="error" :message="errorMessage" />
    </div>

    <!-- Main Content -->
    <div v-else-if="group" class="flex flex-col">
      <!-- Navigation bar -->
      <div class="pt-4 px-4 sm:px-6 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-3 sm:gap-4">
        <BackButton :fallbackRoute="`/groups/${group.uuid}`" label="Back to Group" />

        <button
          @click="showDeleteConfirm = true"
          class="px-3 py-1.5 text-status-error hover:bg-status-error/10 border border-status-error/30 rounded-lg text-sm font-medium transition-colors flex items-center gap-1.5"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
          </svg>
          Delete Group
        </button>
      </div>

      <!-- Main Content -->
      <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
        <!-- Header -->
        <div class="flex items-center gap-3 sm:gap-4">
          <div
            class="w-10 h-10 sm:w-12 sm:h-12 rounded-lg flex items-center justify-center text-white text-lg sm:text-xl font-semibold flex-shrink-0 shadow-sm"
            :style="{ backgroundColor: generalForm.color || '#6366f1', ...colorFilterStyle }"
          >
            {{ group.name.charAt(0).toUpperCase() }}
          </div>
          <div class="min-w-0 flex-1">
            <h1 class="text-xl sm:text-2xl font-semibold text-primary">Group Configuration</h1>
            <p class="text-secondary text-sm mt-0.5">Configure settings and manage membership</p>
          </div>
        </div>

        <!-- Success/Error Messages -->
        <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <!-- Sync Information (if externally synced) -->
        <div v-if="group.external_source" class="bg-surface-alt border border-default rounded-xl p-4">
          <div class="flex items-start gap-3">
            <div class="w-8 h-8 rounded-lg bg-accent/10 inline-flex items-center justify-center flex-shrink-0">
              <svg class="w-4 h-4 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </div>
            <div>
              <h3 class="text-sm font-medium text-primary">Externally Synced Group</h3>
              <p class="text-xs text-secondary mt-1">
                This group is synced from <strong>{{ group.external_source === 'microsoft' ? 'Microsoft Entra ID' : group.external_source }}</strong>.
                Some settings may be managed externally.
              </p>
            </div>
          </div>
        </div>

        <!-- Configuration Cards Grid -->
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 sm:gap-6">
          <!-- General Information -->
          <SectionCard content-padding="p-4">
            <template #title>General Information</template>

            <form @submit.prevent="saveGeneralInfo" class="flex flex-col gap-4">
              <!-- Name -->
              <div>
                <label class="block text-sm font-medium text-primary mb-1">Name</label>
                <input
                  v-model="generalForm.name"
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
                  v-model="generalForm.description"
                  placeholder="Optional description"
                  rows="3"
                  class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
                />
              </div>

              <!-- Color -->
              <div>
                <ColorHueSlider v-model="generalForm.color" label="Color" />
              </div>

              <!-- Save Button -->
              <div class="flex justify-end pt-2">
                <button
                  type="submit"
                  :disabled="saving || !hasGeneralChanges"
                  class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                >
                  <svg v-if="saving" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  Save Changes
                </button>
              </div>
            </form>
          </SectionCard>

          <!-- Members -->
          <SectionCard content-padding="p-4">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Members</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">
                  {{ selectedMemberUuids.length }} selected
                </span>
              </div>
            </template>

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

              <!-- User list -->
              <div class="max-h-64 overflow-y-auto border border-default rounded-lg divide-y divide-default">
                <div
                  v-for="user in filteredUsers"
                  :key="user.uuid"
                  class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                  @click="toggleMember(user.uuid)"
                >
                  <div @click.stop>
                    <Checkbox
                      :model-value="selectedMemberUuids.includes(user.uuid)"
                      @update:model-value="toggleMember(user.uuid)"
                    />
                  </div>
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
                </div>

                <div v-if="filteredUsers.length === 0" class="p-4 text-center text-tertiary text-sm">
                  No users found
                </div>
              </div>

              <!-- Save Button -->
              <div class="flex justify-end pt-2">
                <button
                  @click="saveMembers"
                  :disabled="savingMembers || !hasMemberChanges"
                  class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                >
                  <svg v-if="savingMembers" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  Save Members
                </button>
              </div>
            </div>
          </SectionCard>
        </div>
      </div>
    </div>

    <!-- Not Found -->
    <div v-else class="p-4 sm:p-6 text-center">
      <div class="w-12 h-12 bg-surface-alt rounded-full inline-flex items-center justify-center mx-auto mb-4">
        <svg class="w-6 h-6 shrink-0 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
      </div>
      <p class="text-secondary">Group not found</p>
    </div>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      title="Delete Group"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <div class="flex flex-col gap-4">
        <p class="text-secondary">
          Are you sure you want to delete the group <strong class="text-primary">{{ group?.name }}</strong>?
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
            :disabled="isDeleting"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            <svg v-if="isDeleting" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
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
