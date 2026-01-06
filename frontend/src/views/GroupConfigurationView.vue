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
import { getPaginatedDevices } from '@/services/deviceService';
import { useDataStore } from '@/stores/dataStore';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupDetails, UpdateGroupRequest } from '@/types/group';
import type { User } from '@/types/user';
import type { Device } from '@/types/device';

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

// Devices state
const availableDevices = ref<Device[]>([]);
const selectedDeviceIds = ref<number[]>([]);
const deviceSearchQuery = ref('');
const savingDevices = ref(false);

// Delete confirmation
const showDeleteConfirm = ref(false);
const isDeleting = ref(false);

// Unmanage state
const isUnmanaging = ref(false);

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
  // For externally synced groups, only color can be changed
  if (group.value.external_source) {
    return generalForm.value.color !== (group.value.color || '#6366f1');
  }
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

// Filtered devices based on search
const filteredDevices = computed(() => {
  if (!deviceSearchQuery.value) return availableDevices.value;
  const query = deviceSearchQuery.value.toLowerCase();
  return availableDevices.value.filter(d =>
    d.name.toLowerCase().includes(query) ||
    (d.hostname && d.hostname.toLowerCase().includes(query)) ||
    (d.serial_number && d.serial_number.toLowerCase().includes(query)) ||
    (d.manufacturer && d.manufacturer.toLowerCase().includes(query))
  );
});

// Check if devices have changes
const hasDeviceChanges = computed(() => {
  if (!group.value) return false;
  const currentIds = group.value.devices.map(d => d.id).sort((a, b) => a - b);
  const selectedIds = [...selectedDeviceIds.value].sort((a, b) => a - b);
  return JSON.stringify(currentIds) !== JSON.stringify(selectedIds);
});

// Check if a device is externally synced (from Microsoft)
const isDeviceExternallySynced = (device: Device) => {
  return !!device.intune_device_id || !!device.entra_device_id;
};

// Check if the group itself is externally synced (membership managed externally)
const isExternallySyncedGroup = computed(() => {
  return !!group.value?.external_source;
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

    // Populate selected devices
    selectedDeviceIds.value = group.value.devices.map(d => d.id);
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

// Load available devices
const loadDevices = async () => {
  try {
    const response = await getPaginatedDevices({ page: 1, pageSize: 1000 });
    availableDevices.value = response.data;
  } catch (error) {
    console.error('Failed to load devices:', error);
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

// Toggle device selection
const toggleDevice = (deviceId: number) => {
  const index = selectedDeviceIds.value.indexOf(deviceId);
  if (index === -1) {
    selectedDeviceIds.value.push(deviceId);
  } else {
    selectedDeviceIds.value.splice(index, 1);
  }
};

// Save devices
const saveDevices = async () => {
  if (!group.value) return;

  savingDevices.value = true;
  errorMessage.value = '';

  try {
    await groupService.setGroupDevices(group.value.id, {
      device_ids: selectedDeviceIds.value
    });

    // Reload group to get updated device list
    await loadGroup();

    successMessage.value = 'Devices updated successfully';
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update devices';
  } finally {
    savingDevices.value = false;
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

// Unmanage group (remove from Microsoft sync)
const unmanageGroup = async () => {
  if (!group.value) return;

  const confirmed = confirm(`Are you sure you want to unmanage "${group.value.name}" from Microsoft Entra ID? This will allow manual editing but the group will no longer sync with Microsoft.`);
  if (!confirmed) return;

  isUnmanaging.value = true;
  errorMessage.value = '';

  try {
    await groupService.unmanageGroup(group.value.id);
    // Reload group to get updated data
    await loadGroup();
    successMessage.value = 'Group is now locally managed';
    setTimeout(() => successMessage.value = '', 3000);
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to unmanage group';
  } finally {
    isUnmanaging.value = false;
  }
};

onMounted(() => {
  loadGroup();
  loadUsers();
  loadDevices();
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
        <div v-if="group.external_source" class="bg-accent/10 border border-accent/30 rounded-xl p-4">
          <div class="flex items-start gap-3">
            <div class="w-8 h-8 rounded-lg bg-accent/20 inline-flex items-center justify-center flex-shrink-0">
              <svg class="w-4 h-4 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </div>
            <div class="flex flex-col gap-1">
              <h3 class="text-sm font-medium text-accent">Externally Synced Group</h3>
              <p class="text-xs text-secondary">
                This group is synced from <strong class="text-primary">{{ group.external_source === 'microsoft' ? 'Microsoft Entra ID' : group.external_source }}</strong>.
                Name, description, and membership are managed by Microsoft and updated during sync.
              </p>
              <p v-if="group.last_synced_at" class="text-xs text-tertiary">
                Last synced: {{ new Date(group.last_synced_at).toLocaleString() }}
              </p>
              <button
                @click="unmanageGroup"
                :disabled="isUnmanaging"
                class="mt-2 flex items-center gap-2 px-3 py-1.5 bg-status-warning/20 text-status-warning rounded-lg hover:bg-status-warning/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-xs font-medium self-start"
              >
                <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
                  <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
                  <path d="M8 2v3" />
                  <path d="M2 8h3" />
                  <path d="M16 22v-3" />
                  <path d="M22 16h-3" />
                </svg>
                {{ isUnmanaging ? 'Processing...' : 'Unmanage from Microsoft' }}
              </button>
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
                  :disabled="isExternallySyncedGroup"
                  class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent disabled:opacity-60 disabled:cursor-not-allowed"
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
                  :disabled="isExternallySyncedGroup"
                  class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none disabled:opacity-60 disabled:cursor-not-allowed"
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
                  {{ isExternallySyncedGroup ? group.members.length : selectedMemberUuids.length }} {{ isExternallySyncedGroup ? 'members' : 'selected' }}
                </span>
              </div>
            </template>

            <!-- Read-only view for externally synced groups -->
            <div v-if="isExternallySyncedGroup">
              <div v-if="group.members.length > 0" class="max-h-64 overflow-y-auto border border-default rounded-lg divide-y divide-default">
                <div
                  v-for="member in group.members"
                  :key="member.uuid"
                  class="flex items-center gap-3 p-2.5 sm:p-3"
                >
                  <UserAvatar
                    :name="member.uuid"
                    :userName="member.name"
                    :avatar="member.avatar_thumb || member.avatar_url"
                    size="sm"
                    :clickable="true"
                    :show-name="false"
                  />
                  <div class="flex-1 min-w-0">
                    <div class="text-sm font-medium text-primary truncate">{{ member.name }}</div>
                  </div>
                </div>
              </div>
              <p v-else class="text-tertiary text-sm">No members</p>
            </div>

            <!-- Editable view for local groups -->
            <div v-else class="flex flex-col gap-4">
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

        <!-- Devices Section (Full Width) -->
        <SectionCard content-padding="p-4">
          <template #title>
            <div class="flex items-center justify-between">
              <span>Devices</span>
              <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">
                {{ isExternallySyncedGroup ? group.devices.length : selectedDeviceIds.length }} {{ isExternallySyncedGroup ? 'devices' : 'selected' }}
              </span>
            </div>
          </template>

          <!-- Read-only view for externally synced groups -->
          <div v-if="isExternallySyncedGroup">
            <div v-if="group.devices.length > 0" class="max-h-72 overflow-y-auto border border-default rounded-lg divide-y divide-default">
              <div
                v-for="device in group.devices"
                :key="device.id"
                class="flex items-center gap-3 p-2.5 sm:p-3"
              >
                <!-- Device icon based on OS -->
                <div class="w-8 h-8 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                  <svg v-if="device.operating_system?.toLowerCase().includes('windows')" class="w-4 h-4 text-accent" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-12.9-1.801"/>
                  </svg>
                  <svg v-else-if="device.operating_system?.toLowerCase().includes('mac') || device.operating_system?.toLowerCase().includes('ios')" class="w-4 h-4 text-tertiary" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M13 3.5c.73-.83 1.94-1.46 2.94-1.5.13 1.17-.34 2.35-1.04 3.19-.69.85-1.83 1.51-2.95 1.42-.15-1.15.41-2.35 1.05-3.11z"/>
                  </svg>
                  <svg v-else-if="device.operating_system?.toLowerCase().includes('linux') || device.operating_system?.toLowerCase().includes('ubuntu')" class="w-4 h-4 text-tertiary" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M12.504 0c-.155 0-.315.008-.48.021-4.226.333-3.105 4.807-3.17 6.298-.076 1.092-.3 1.953-1.05 3.02-.885 1.051-2.127 2.75-2.716 4.521-.278.832-.41 1.684-.287 2.489a.424.424 0 00-.11.135c-.26.268-.45.6-.663.839-.199.199-.485.267-.797.4-.313.136-.658.269-.864.68-.09.189-.136.394-.132.602 0 .199.027.4.055.536.058.399.116.728.04.97-.249.68-.28 1.145-.106 1.484.174.334.535.47.94.601.81.2 1.91.135 2.774.6.926.466 1.866.67 2.616.47.526-.116.97-.464 1.208-.946.587-.003 1.23-.269 2.26-.334.699-.058 1.574.267 2.577.2.025.134.063.198.114.333l.003.003c.391.778 1.113 1.132 1.884 1.071.771-.06 1.592-.536 2.257-1.306.631-.765 1.683-1.084 2.378-1.503.348-.199.629-.469.649-.853.023-.4-.2-.811-.714-1.376v-.097l-.003-.003c-.17-.2-.25-.535-.338-.926-.085-.401-.182-.786-.492-1.046h-.003c-.059-.054-.123-.067-.188-.135a.357.357 0 00-.19-.064c.431-1.278.264-2.55-.173-3.694-.533-1.41-1.465-2.638-2.175-3.483-.796-1.005-1.576-1.957-1.56-3.368.026-2.152.236-6.133-3.544-6.139zm.529 3.405h.013c.213 0 .396.062.584.198.19.135.33.332.438.533.105.259.158.459.166.724 0-.02.006-.04.006-.06v.105a.086.086 0 01-.004-.021l-.004-.024a1.807 1.807 0 01-.15.706.953.953 0 01-.213.335.71.71 0 00-.088-.042c-.104-.045-.198-.064-.284-.133a1.312 1.312 0 00-.22-.066c.05-.06.146-.133.183-.198.053-.128.082-.264.088-.402v-.02a1.21 1.21 0 00-.061-.4c-.045-.134-.101-.2-.183-.333-.084-.066-.167-.132-.267-.132h-.016c-.093 0-.176.03-.262.132a.8.8 0 00-.205.334 1.18 1.18 0 00-.09.468v.021c.003.098.008.197.03.297a.9.9 0 01-.132-.168c-.137-.256-.21-.524-.21-.795v-.105c0-.097.003-.133.01-.267.038-.2.103-.4.2-.533.096-.135.22-.2.368-.2zm-1.922.198c.145 0 .282.058.398.198.12.135.204.266.246.4.09.27.135.54.15.806v.124c-.007.333-.07.666-.168.866a.43.43 0 01-.062.132.71.71 0 00-.254-.334c-.106-.066-.18-.135-.313-.135h-.016c-.12 0-.225.07-.32.135a.8.8 0 00-.172.334 1.13 1.13 0 00-.1.468v.021c.007.133.027.267.057.4.03.124.075.24.133.328a1.05 1.05 0 01-.263-.4 1.807 1.807 0 01-.145-.801v-.2c0-.135.003-.2.016-.268a1.94 1.94 0 01.35-.867c.152-.2.358-.332.533-.332z"/>
                  </svg>
                  <svg v-else class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
                </div>

                <div class="flex-1 min-w-0">
                  <div class="text-sm font-medium text-primary truncate">{{ device.name }}</div>
                  <div class="text-xs text-tertiary truncate">
                    <template v-if="device.manufacturer || device.model">
                      {{ [device.manufacturer, device.model].filter(Boolean).join(' ') }}
                    </template>
                    <template v-if="device.serial_number">
                      <span v-if="device.manufacturer || device.model"> · </span>SN: {{ device.serial_number }}
                    </template>
                    <template v-if="!device.manufacturer && !device.model && !device.serial_number && device.operating_system">
                      {{ device.operating_system }}
                    </template>
                  </div>
                </div>
              </div>
            </div>
            <p v-else class="text-tertiary text-sm">No devices</p>
          </div>

          <!-- Editable view for local groups -->
          <div v-else class="flex flex-col gap-4">
            <!-- Search -->
            <div class="relative">
              <svg xmlns="http://www.w3.org/2000/svg" class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-tertiary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <input
                v-model="deviceSearchQuery"
                type="text"
                placeholder="Search devices by name, hostname, serial number..."
                class="w-full pl-10 pr-4 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
              />
            </div>

            <!-- Device list -->
            <div class="max-h-72 overflow-y-auto border border-default rounded-lg divide-y divide-default">
              <div
                v-for="device in filteredDevices"
                :key="device.id"
                class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                @click="toggleDevice(device.id)"
              >
                <div @click.stop>
                  <Checkbox
                    :model-value="selectedDeviceIds.includes(device.id)"
                    @update:model-value="toggleDevice(device.id)"
                  />
                </div>

                <!-- Device icon based on OS -->
                <div class="w-8 h-8 rounded-lg bg-surface-alt flex items-center justify-center flex-shrink-0">
                  <svg v-if="device.operating_system?.toLowerCase().includes('windows')" class="w-4 h-4 text-accent" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-12.9-1.801"/>
                  </svg>
                  <svg v-else-if="device.operating_system?.toLowerCase().includes('mac') || device.operating_system?.toLowerCase().includes('ios')" class="w-4 h-4 text-tertiary" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M13 3.5c.73-.83 1.94-1.46 2.94-1.5.13 1.17-.34 2.35-1.04 3.19-.69.85-1.83 1.51-2.95 1.42-.15-1.15.41-2.35 1.05-3.11z"/>
                  </svg>
                  <svg v-else-if="device.operating_system?.toLowerCase().includes('linux') || device.operating_system?.toLowerCase().includes('ubuntu')" class="w-4 h-4 text-tertiary" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M12.504 0c-.155 0-.315.008-.48.021-4.226.333-3.105 4.807-3.17 6.298-.076 1.092-.3 1.953-1.05 3.02-.885 1.051-2.127 2.75-2.716 4.521-.278.832-.41 1.684-.287 2.489a.424.424 0 00-.11.135c-.26.268-.45.6-.663.839-.199.199-.485.267-.797.4-.313.136-.658.269-.864.68-.09.189-.136.394-.132.602 0 .199.027.4.055.536.058.399.116.728.04.97-.249.68-.28 1.145-.106 1.484.174.334.535.47.94.601.81.2 1.91.135 2.774.6.926.466 1.866.67 2.616.47.526-.116.97-.464 1.208-.946.587-.003 1.23-.269 2.26-.334.699-.058 1.574.267 2.577.2.025.134.063.198.114.333l.003.003c.391.778 1.113 1.132 1.884 1.071.771-.06 1.592-.536 2.257-1.306.631-.765 1.683-1.084 2.378-1.503.348-.199.629-.469.649-.853.023-.4-.2-.811-.714-1.376v-.097l-.003-.003c-.17-.2-.25-.535-.338-.926-.085-.401-.182-.786-.492-1.046h-.003c-.059-.054-.123-.067-.188-.135a.357.357 0 00-.19-.064c.431-1.278.264-2.55-.173-3.694-.533-1.41-1.465-2.638-2.175-3.483-.796-1.005-1.576-1.957-1.56-3.368.026-2.152.236-6.133-3.544-6.139zm.529 3.405h.013c.213 0 .396.062.584.198.19.135.33.332.438.533.105.259.158.459.166.724 0-.02.006-.04.006-.06v.105a.086.086 0 01-.004-.021l-.004-.024a1.807 1.807 0 01-.15.706.953.953 0 01-.213.335.71.71 0 00-.088-.042c-.104-.045-.198-.064-.284-.133a1.312 1.312 0 00-.22-.066c.05-.06.146-.133.183-.198.053-.128.082-.264.088-.402v-.02a1.21 1.21 0 00-.061-.4c-.045-.134-.101-.2-.183-.333-.084-.066-.167-.132-.267-.132h-.016c-.093 0-.176.03-.262.132a.8.8 0 00-.205.334 1.18 1.18 0 00-.09.468v.021c.003.098.008.197.03.297a.9.9 0 01-.132-.168c-.137-.256-.21-.524-.21-.795v-.105c0-.097.003-.133.01-.267.038-.2.103-.4.2-.533.096-.135.22-.2.368-.2zm-1.922.198c.145 0 .282.058.398.198.12.135.204.266.246.4.09.27.135.54.15.806v.124c-.007.333-.07.666-.168.866a.43.43 0 01-.062.132.71.71 0 00-.254-.334c-.106-.066-.18-.135-.313-.135h-.016c-.12 0-.225.07-.32.135a.8.8 0 00-.172.334 1.13 1.13 0 00-.1.468v.021c.007.133.027.267.057.4.03.124.075.24.133.328a1.05 1.05 0 01-.263-.4 1.807 1.807 0 01-.145-.801v-.2c0-.135.003-.2.016-.268a1.94 1.94 0 01.35-.867c.152-.2.358-.332.533-.332z"/>
                  </svg>
                  <svg v-else class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                  </svg>
                </div>

                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="text-sm font-medium text-primary truncate">{{ device.name }}</span>
                    <!-- External sync indicator -->
                    <span
                      v-if="isDeviceExternallySynced(device)"
                      class="px-1.5 py-0.5 text-xs bg-accent/10 text-accent rounded-full flex items-center gap-1"
                      title="Synced from Microsoft Intune"
                    >
                      <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                      </svg>
                      Synced
                    </span>
                  </div>
                  <div class="text-xs text-tertiary truncate">
                    <template v-if="device.manufacturer || device.model">
                      {{ [device.manufacturer, device.model].filter(Boolean).join(' ') }}
                    </template>
                    <template v-if="device.serial_number">
                      <span v-if="device.manufacturer || device.model"> · </span>SN: {{ device.serial_number }}
                    </template>
                    <template v-if="!device.manufacturer && !device.model && !device.serial_number && device.operating_system">
                      {{ device.operating_system }}
                    </template>
                  </div>
                </div>
              </div>

              <div v-if="filteredDevices.length === 0" class="p-4 text-center text-tertiary text-sm">
                No devices found
              </div>
            </div>

            <!-- Save Button -->
            <div class="flex justify-end pt-2">
              <button
                @click="saveDevices"
                :disabled="savingDevices || !hasDeviceChanges"
                class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
              >
                <svg v-if="savingDevices" class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                Save Devices
              </button>
            </div>
          </div>
        </SectionCard>
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
