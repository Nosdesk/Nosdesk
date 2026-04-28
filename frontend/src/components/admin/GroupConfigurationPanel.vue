<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import Modal from '@/components/Modal.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import DeviceOsIcon from '@/components/common/DeviceOsIcon.vue';
import Icon from '@/components/common/Icon.vue';
import Spinner from '@/components/common/Spinner.vue';
import { groupService } from '@/services/groupService';
import { getPaginatedDevices } from '@/services/deviceService';
import { useDataStore } from '@/stores/dataStore';
import { useColorFilter } from '@/composables/useColorFilter';
import type { GroupDetails, GroupWithMemberCount, UpdateGroupRequest } from '@/types/group';
import type { User } from '@/types/user';
import type { Device } from '@/types/device';

const props = defineProps<{
  groupUuid: string;
}>();

const emit = defineEmits<{
  close: [];
  deleted: [];
  updated: [];
}>();

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

// Includes state
const availableGroups = ref<GroupWithMemberCount[]>([]);
const selectedIncludeIds = ref<number[]>([]);
const includeSearchQuery = ref('');
const savingIncludes = ref(false);

// Delete confirmation
const showDeleteConfirm = ref(false);
const isDeleting = ref(false);

// Unmanage state
const isUnmanaging = ref(false);

// Filtered users based on search
const filteredUsers = computed(() => {
  let users = availableUsers.value;
  if (userSearchQuery.value) {
    const query = userSearchQuery.value.toLowerCase();
    users = users.filter(u =>
      u.name.toLowerCase().includes(query) ||
      (u.email && u.email.toLowerCase().includes(query))
    );
  }
  return users;
});

// Users categorized into three groups for the picker
const categorizedUsers = computed(() => {
  const assigned: typeof filteredUsers.value = [];
  const included: typeof filteredUsers.value = [];
  const other: typeof filteredUsers.value = [];

  for (const user of filteredUsers.value) {
    if (selectedMemberUuids.value.includes(user.uuid)) {
      assigned.push(user);
    } else if (effectiveGroupSources.value.has(user.uuid)) {
      included.push(user);
    } else {
      other.push(user);
    }
  }

  return { assigned, included, other };
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

// Filtered available groups for includes (exclude self, show only other groups)
const filteredAvailableGroups = computed(() => {
  if (!group.value) return [];
  let result = availableGroups.value.filter(g => g.id !== group.value!.id);
  if (includeSearchQuery.value) {
    const query = includeSearchQuery.value.toLowerCase();
    result = result.filter(g =>
      g.name.toLowerCase().includes(query) ||
      (g.description && g.description.toLowerCase().includes(query))
    );
  }
  return result;
});

// Check if includes have changes
const hasIncludeChanges = computed(() => {
  if (!group.value) return false;
  const currentIds = group.value.included_groups.map(g => g.id).sort((a, b) => a - b);
  const selectedIds = [...selectedIncludeIds.value].sort((a, b) => a - b);
  return JSON.stringify(currentIds) !== JSON.stringify(selectedIds);
});

// Consolidated members list: direct + effective from included groups, deduplicated with attribution
const consolidatedMembers = computed(() => {
  if (!group.value) return [];

  const memberMap = new Map<string, {
    uuid: string;
    name: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
    sources: Array<{ type: 'direct' } | { type: 'group'; name: string; color: string }>;
  }>();

  // Direct members
  for (const m of group.value.members) {
    memberMap.set(m.uuid, { ...m, sources: [{ type: 'direct' }] });
  }

  // Members from included groups
  for (const g of group.value.included_groups) {
    if (!g.members) continue;
    for (const m of g.members) {
      const existing = memberMap.get(m.uuid);
      if (existing) {
        existing.sources.push({ type: 'group', name: g.name, color: g.color || '#6366f1' });
      } else {
        memberMap.set(m.uuid, {
          ...m,
          sources: [{ type: 'group', name: g.name, color: g.color || '#6366f1' }],
        });
      }
    }
  }

  return Array.from(memberMap.values());
});

// Map of user UUID -> included group sources, for showing attribution in the picker
const effectiveGroupSources = computed(() => {
  if (!group.value) return new Map<string, Array<{ name: string; color: string }>>();

  const map = new Map<string, Array<{ name: string; color: string }>>();
  for (const g of group.value.included_groups) {
    if (!g.members) continue;
    for (const m of g.members) {
      const existing = map.get(m.uuid);
      if (existing) {
        existing.push({ name: g.name, color: g.color || '#6366f1' });
      } else {
        map.set(m.uuid, [{ name: g.name, color: g.color || '#6366f1' }]);
      }
    }
  }
  return map;
});

// Check if a device is externally synced (from Microsoft)
const isDeviceExternallySynced = (device: Device) => {
  return !!device.intune_device_id || !!device.entra_device_id;
};

// Check if the group itself is externally synced (membership managed externally)
const isExternallySyncedGroup = computed(() => {
  return !!group.value?.external_source;
});

// Load group data. When `silent` is true, skip the loading skeleton (used when switching between groups).
const loadGroup = async (silent = false) => {
  try {
    if (!silent) loading.value = true;
    errorMessage.value = '';

    if (!props.groupUuid) {
      errorMessage.value = 'Invalid group ID';
      loading.value = false;
      return;
    }

    group.value = await groupService.getGroupDetails(props.groupUuid);

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

    // Populate selected includes
    selectedIncludeIds.value = group.value.included_groups.map(g => g.id);
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

// Load available groups for includes
const loadAvailableGroups = async () => {
  try {
    availableGroups.value = await groupService.getGroups();
  } catch (error) {
    console.error('Failed to load available groups:', error);
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
    emit('updated');
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
    emit('updated');
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
    emit('updated');
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update devices';
  } finally {
    savingDevices.value = false;
  }
};

// Toggle include selection
const toggleInclude = (groupId: number) => {
  const index = selectedIncludeIds.value.indexOf(groupId);
  if (index === -1) {
    selectedIncludeIds.value.push(groupId);
  } else {
    selectedIncludeIds.value.splice(index, 1);
  }
};

// Save includes
const saveIncludes = async () => {
  if (!group.value) return;

  savingIncludes.value = true;
  errorMessage.value = '';

  try {
    await groupService.setGroupIncludes(group.value.id, {
      child_group_ids: selectedIncludeIds.value
    });

    // Reload group to get updated include list
    await loadGroup();

    successMessage.value = 'Included groups updated successfully';
    setTimeout(() => successMessage.value = '', 3000);
    emit('updated');
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to update included groups';
  } finally {
    savingIncludes.value = false;
  }
};

// Delete group
const deleteGroup = async () => {
  if (!group.value) return;

  isDeleting.value = true;
  errorMessage.value = '';

  try {
    await groupService.deleteGroup(group.value.id);
    emit('deleted');
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to delete group';
    showDeleteConfirm.value = false;
  } finally {
    isDeleting.value = false;
  }
};

// Unmanage group (remove from Microsoft sync)
const showUnmanageConfirm = ref(false);

const unmanageGroup = () => {
  if (!group.value) return;
  showUnmanageConfirm.value = true;
};

const doUnmanageGroup = async () => {
  showUnmanageConfirm.value = false;
  if (!group.value) return;
  isUnmanaging.value = true;
  errorMessage.value = '';

  try {
    await groupService.unmanageGroup(group.value.id);
    // Reload group to get updated data
    await loadGroup();
    successMessage.value = 'Group is now locally managed';
    setTimeout(() => successMessage.value = '', 3000);
    emit('updated');
  } catch (error) {
    const axiosError = error as { response?: { data?: { message?: string } } };
    errorMessage.value = axiosError.response?.data?.message || 'Failed to unmanage group';
  } finally {
    isUnmanaging.value = false;
  }
};

// Reload when groupUuid changes (e.g. selecting a different group in the list)
watch(() => props.groupUuid, () => {
  errorMessage.value = '';
  successMessage.value = '';
  userSearchQuery.value = '';
  deviceSearchQuery.value = '';
  includeSearchQuery.value = '';
  loadGroup(!!group.value);
});

onMounted(() => {
  loadGroup();
  loadUsers();
  loadDevices();
  loadAvailableGroups();
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Loading State with Skeleton -->
    <div v-if="loading" class="flex flex-col p-4 sm:p-6 gap-4">
      <!-- Skeleton Header -->
      <div class="flex items-center justify-between">
        <div class="h-7 w-56 bg-surface-alt rounded animate-pulse"></div>
        <div class="h-8 w-8 bg-surface-alt rounded-lg animate-pulse"></div>
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

    <!-- Error State -->
    <div v-else-if="errorMessage && !group" class="p-4 sm:p-6">
      <AlertMessage type="error" :message="errorMessage" />
    </div>

    <!-- Main Content -->
    <div v-else-if="group" class="flex flex-col flex-1 overflow-y-auto">
      <!-- Panel Header -->
      <div class="sticky top-0 z-10 bg-app border-b border-default px-4 sm:px-6 py-3 flex items-center justify-between gap-3 flex-shrink-0">
        <div class="flex items-center gap-3 min-w-0">
          <div
            class="w-8 h-8 rounded-lg flex items-center justify-center text-white text-sm font-semibold flex-shrink-0 shadow-sm"
            :style="{ backgroundColor: generalForm.color || '#6366f1', ...colorFilterStyle }"
          >
            {{ group.name.charAt(0).toUpperCase() }}
          </div>
          <div class="min-w-0">
            <h2 class="text-lg font-semibold text-primary truncate">{{ group.name }}</h2>
            <p class="text-xs text-secondary">Group Configuration</p>
          </div>
        </div>
        <div class="flex items-center gap-2 flex-shrink-0">
          <button
            @click="showDeleteConfirm = true"
            class="p-1.5 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
            title="Delete group"
          >
            <Icon name="trash" />
          </button>
          <button
            @click="emit('close')"
            class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
            title="Close panel"
          >
            <Icon name="close" size="md" />
          </button>
        </div>
      </div>

      <!-- Scrollable Content -->
      <div class="flex flex-col gap-4 sm:gap-6 px-4 sm:px-6 py-4">
        <!-- Success/Error Messages -->
        <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <!-- MANAGED GROUP: Read-only view -->
        <template v-if="isExternallySyncedGroup">
          <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-3 px-4 py-3 bg-surface border border-default rounded-xl">
            <div class="flex items-center gap-3 min-w-0">
              <span class="text-tertiary flex-shrink-0 inline-flex">
                <Icon name="refresh" size="md" />
              </span>
              <div class="min-w-0">
                <p class="text-sm font-medium text-primary truncate">Managed by {{ group.external_source === 'microsoft' ? 'Microsoft Entra ID' : group.external_source }}</p>
                <p v-if="group.last_synced_at" class="text-xs text-tertiary">Last synced {{ new Date(group.last_synced_at).toLocaleDateString() }}</p>
              </div>
            </div>
            <div class="flex items-center gap-3 text-sm flex-shrink-0">
              <button
                @click="unmanageGroup"
                :disabled="isUnmanaging"
                class="text-secondary hover:text-primary transition-colors disabled:opacity-50"
              >
                {{ isUnmanaging ? 'Processing...' : 'Unmanage' }}
              </button>
              <span class="text-tertiary">|</span>
              <router-link to="/admin/microsoft-graph" class="text-accent hover:underline whitespace-nowrap">
                Sync Settings
              </router-link>
            </div>
          </div>

          <p v-if="group.description" class="text-sm text-secondary">{{ group.description }}</p>

          <!-- Members (consolidated: direct + effective) -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Members</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ consolidatedMembers.length }}</span>
              </div>
            </template>
            <div v-if="consolidatedMembers.length > 0" class="divide-y divide-default max-h-80 overflow-y-auto">
              <div v-for="member in consolidatedMembers" :key="member.uuid" class="flex items-center gap-3 px-4 py-2.5">
                <UserAvatar :name="member.uuid" :userName="member.name" :avatar="member.avatar_thumb || member.avatar_url" size="sm" :clickable="true" :show-name="false" />
                <div class="flex-1 min-w-0">
                  <div class="text-sm text-primary truncate">{{ member.name }}</div>
                  <div class="flex items-center gap-1.5 mt-0.5">
                    <template v-for="(source, i) in member.sources" :key="i">
                      <span v-if="source.type === 'direct'" class="text-xs text-tertiary">Direct</span>
                      <span v-else class="inline-flex items-center gap-1 text-xs text-secondary">
                        <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" :style="{ backgroundColor: source.color }"></span>
                        {{ source.name }}
                      </span>
                      <span v-if="i < member.sources.length - 1" class="text-xs text-tertiary">&middot;</span>
                    </template>
                  </div>
                </div>
              </div>
            </div>
            <p v-else class="px-4 py-3 text-tertiary text-sm">No members</p>
          </SectionCard>

          <!-- Devices -->
          <SectionCard content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Devices</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ group.devices.length }}</span>
              </div>
            </template>
            <div v-if="group.devices.length > 0" class="divide-y divide-default max-h-72 overflow-y-auto">
              <div v-for="device in group.devices" :key="device.id" class="flex items-center gap-3 px-4 py-2.5">
                <DeviceOsIcon :os="device.operating_system" />
                <div class="flex-1 min-w-0">
                  <div class="text-sm text-primary truncate">{{ device.name }}</div>
                  <div class="text-xs text-tertiary truncate">
                    <template v-if="device.manufacturer || device.model">{{ [device.manufacturer, device.model].filter(Boolean).join(' ') }}</template>
                    <template v-if="device.serial_number"><span v-if="device.manufacturer || device.model"> · </span>SN: {{ device.serial_number }}</template>
                    <template v-if="!device.manufacturer && !device.model && !device.serial_number && device.operating_system">{{ device.operating_system }}</template>
                  </div>
                </div>
              </div>
            </div>
            <p v-else class="px-4 py-3 text-tertiary text-sm">No devices</p>
          </SectionCard>

          <!-- Included In (read-only) -->
          <SectionCard v-if="group.included_in && group.included_in.length > 0" content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Included In</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ group.included_in.length }}</span>
              </div>
            </template>
            <div class="divide-y divide-default max-h-48 overflow-y-auto">
              <div v-for="parentGroup in group.included_in" :key="parentGroup.id" class="flex items-center gap-3 px-4 py-2.5">
                <div
                  class="w-6 h-6 rounded flex items-center justify-center text-white text-xs font-semibold flex-shrink-0"
                  :style="{ backgroundColor: parentGroup.color || '#6366f1', ...colorFilterStyle }"
                >
                  {{ parentGroup.name.charAt(0).toUpperCase() }}
                </div>
                <span class="text-sm text-primary truncate flex-1">{{ parentGroup.name }}</span>
                <span class="text-xs text-tertiary">{{ parentGroup.member_count }} member{{ parentGroup.member_count !== 1 ? 's' : '' }}</span>
              </div>
            </div>
          </SectionCard>
        </template>

        <!-- LOCAL GROUP: Editable forms -->
        <template v-else>
          <div class="grid grid-cols-1 lg:grid-cols-2 gap-4 sm:gap-6">
            <!-- General Information -->
            <SectionCard content-padding="p-4">
              <template #title>General Information</template>
              <form @submit.prevent="saveGeneralInfo" class="flex flex-col gap-4">
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
                <div>
                  <label class="block text-sm font-medium text-primary mb-1">Description</label>
                  <textarea
                    v-model="generalForm.description"
                    placeholder="Optional description"
                    rows="3"
                    class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
                  />
                </div>
                <div>
                  <ColorHueSlider v-model="generalForm.color" label="Color" />
                </div>
                <div class="flex justify-end pt-2">
                  <button
                    type="submit"
                    :disabled="saving || !hasGeneralChanges"
                    class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                  >
                    <Spinner v-if="saving" />
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
                  <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ selectedMemberUuids.length + categorizedUsers.included.length }} / {{ availableUsers.length }}</span>
                </div>
              </template>
              <div class="flex flex-col gap-4">
                <div class="relative">
                  <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
                    <Icon name="search" />
                  </span>
                  <input
                    v-model="userSearchQuery"
                    type="text"
                    placeholder="Search users..."
                    class="w-full pl-10 pr-4 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                  />
                </div>
                <div class="max-h-72 overflow-y-auto border border-default rounded-lg">
                  <!-- Assigned (explicitly selected) -->
                  <template v-if="categorizedUsers.assigned.length > 0">
                    <div class="px-3 py-1.5 bg-surface-alt text-xs font-medium text-secondary uppercase tracking-wide sticky top-0 z-[1] border-b border-default">Assigned <span class="text-tertiary font-normal normal-case">({{ categorizedUsers.assigned.length }})</span></div>
                    <div class="divide-y divide-default">
                      <div
                        v-for="user in categorizedUsers.assigned"
                        :key="user.uuid"
                        class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                        @click="toggleMember(user.uuid)"
                      >
                        <div @click.stop>
                          <Checkbox :model-value="true" @update:model-value="toggleMember(user.uuid)" />
                        </div>
                        <UserAvatar :name="user.uuid" :userName="user.name" :avatar="user.avatar_thumb || user.avatar_url" size="sm" :clickable="false" :show-name="false" />
                        <div class="flex-1 min-w-0">
                          <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
                          <div v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</div>
                          <div v-if="effectiveGroupSources.has(user.uuid)" class="flex items-center gap-1.5 mt-0.5">
                            <span class="text-xs text-tertiary">also via</span>
                            <span v-for="(source, i) in effectiveGroupSources.get(user.uuid)" :key="i" class="inline-flex items-center gap-1 text-xs text-secondary">
                              <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" :style="{ backgroundColor: source.color }"></span>
                              {{ source.name }}<span v-if="i < effectiveGroupSources.get(user.uuid)!.length - 1" class="text-tertiary">,</span>
                            </span>
                          </div>
                        </div>
                      </div>
                    </div>
                  </template>

                  <!-- Included via groups (not explicitly assigned, but effectively members) -->
                  <template v-if="categorizedUsers.included.length > 0">
                    <div class="px-3 py-1.5 bg-surface-alt text-xs font-medium text-secondary uppercase tracking-wide sticky top-0 z-[1] border-y border-default">Included via Groups <span class="text-tertiary font-normal normal-case">({{ categorizedUsers.included.length }})</span></div>
                    <div class="divide-y divide-default">
                      <div
                        v-for="user in categorizedUsers.included"
                        :key="user.uuid"
                        class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors bg-accent/[0.03] border-l-2 border-l-accent/30"
                        @click="toggleMember(user.uuid)"
                      >
                        <div @click.stop>
                          <Checkbox :model-value="false" @update:model-value="toggleMember(user.uuid)" />
                        </div>
                        <UserAvatar :name="user.uuid" :userName="user.name" :avatar="user.avatar_thumb || user.avatar_url" size="sm" :clickable="false" :show-name="false" />
                        <div class="flex-1 min-w-0">
                          <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
                          <div v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</div>
                          <div class="flex items-center gap-1.5 mt-0.5">
                            <span class="text-xs text-tertiary">via</span>
                            <span v-for="(source, i) in effectiveGroupSources.get(user.uuid)" :key="i" class="inline-flex items-center gap-1 text-xs text-secondary">
                              <span class="w-2.5 h-2.5 rounded-full flex-shrink-0" :style="{ backgroundColor: source.color }"></span>
                              {{ source.name }}<span v-if="i < effectiveGroupSources.get(user.uuid)!.length - 1" class="text-tertiary">,</span>
                            </span>
                          </div>
                        </div>
                      </div>
                    </div>
                  </template>

                  <!-- Not assigned -->
                  <template v-if="categorizedUsers.other.length > 0">
                    <div class="px-3 py-1.5 bg-surface-alt text-xs font-medium text-tertiary uppercase tracking-wide sticky top-0 z-[1] border-y border-default">Not Assigned <span class="font-normal normal-case">({{ categorizedUsers.other.length }})</span></div>
                    <div class="divide-y divide-default">
                      <div
                        v-for="user in categorizedUsers.other"
                        :key="user.uuid"
                        class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                        @click="toggleMember(user.uuid)"
                      >
                        <div @click.stop>
                          <Checkbox :model-value="false" @update:model-value="toggleMember(user.uuid)" />
                        </div>
                        <UserAvatar :name="user.uuid" :userName="user.name" :avatar="user.avatar_thumb || user.avatar_url" size="sm" :clickable="false" :show-name="false" />
                        <div class="flex-1 min-w-0">
                          <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
                          <div v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</div>
                        </div>
                      </div>
                    </div>
                  </template>

                  <div v-if="filteredUsers.length === 0" class="p-4 text-center text-tertiary text-sm">No users found</div>
                </div>
                <div class="flex justify-end pt-2">
                  <button
                    @click="saveMembers"
                    :disabled="savingMembers || !hasMemberChanges"
                    class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                  >
                    <Spinner v-if="savingMembers" />
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
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ selectedDeviceIds.length }} selected</span>
              </div>
            </template>
            <div class="flex flex-col gap-4">
              <div class="relative">
                <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
                  <Icon name="search" />
                </span>
                <input
                  v-model="deviceSearchQuery"
                  type="text"
                  placeholder="Search devices by name, hostname, serial number..."
                  class="w-full pl-10 pr-4 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                />
              </div>
              <div class="max-h-72 overflow-y-auto border border-default rounded-lg divide-y divide-default">
                <div
                  v-for="device in filteredDevices"
                  :key="device.id"
                  class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                  @click="toggleDevice(device.id)"
                >
                  <div @click.stop>
                    <Checkbox :model-value="selectedDeviceIds.includes(device.id)" @update:model-value="toggleDevice(device.id)" />
                  </div>
                  <DeviceOsIcon :os="device.operating_system" />
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-primary truncate">{{ device.name }}</span>
                      <span
                        v-if="isDeviceExternallySynced(device)"
                        class="px-1.5 py-0.5 text-xs bg-accent/10 text-accent rounded-full flex items-center gap-1"
                        title="Synced from Microsoft Intune"
                      >
                        <Icon name="refresh" size="xs" />
                        Synced
                      </span>
                    </div>
                    <div class="text-xs text-tertiary truncate">
                      <template v-if="device.manufacturer || device.model">{{ [device.manufacturer, device.model].filter(Boolean).join(' ') }}</template>
                      <template v-if="device.serial_number"><span v-if="device.manufacturer || device.model"> · </span>SN: {{ device.serial_number }}</template>
                      <template v-if="!device.manufacturer && !device.model && !device.serial_number && device.operating_system">{{ device.operating_system }}</template>
                    </div>
                  </div>
                </div>
                <div v-if="filteredDevices.length === 0" class="p-4 text-center text-tertiary text-sm">No devices found</div>
              </div>
              <div class="flex justify-end pt-2">
                <button
                  @click="saveDevices"
                  :disabled="savingDevices || !hasDeviceChanges"
                  class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                >
                  <Spinner v-if="savingDevices" />
                  Save Devices
                </button>
              </div>
            </div>
          </SectionCard>

          <!-- Included Groups Section (Full Width) -->
          <SectionCard content-padding="p-4">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Included Groups</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ selectedIncludeIds.length }} selected</span>
              </div>
            </template>
            <div class="flex flex-col gap-4">
              <p class="text-xs text-tertiary">Members of included groups are treated as members of this group for visibility, access, and assignment.</p>
              <div class="relative">
                <span class="absolute left-3 top-1/2 -translate-y-1/2 text-tertiary inline-flex">
                  <Icon name="search" />
                </span>
                <input
                  v-model="includeSearchQuery"
                  type="text"
                  placeholder="Search groups..."
                  class="w-full pl-10 pr-4 py-2 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                />
              </div>
              <div class="max-h-64 overflow-y-auto border border-default rounded-lg divide-y divide-default">
                <div
                  v-for="availableGroup in filteredAvailableGroups"
                  :key="availableGroup.id"
                  class="flex items-center gap-3 p-2.5 sm:p-3 hover:bg-surface-hover cursor-pointer transition-colors"
                  @click="toggleInclude(availableGroup.id)"
                >
                  <div @click.stop>
                    <Checkbox :model-value="selectedIncludeIds.includes(availableGroup.id)" @update:model-value="toggleInclude(availableGroup.id)" />
                  </div>
                  <div
                    class="w-6 h-6 rounded flex items-center justify-center text-white text-xs font-semibold flex-shrink-0"
                    :style="{ backgroundColor: availableGroup.color || '#6366f1', ...colorFilterStyle }"
                  >
                    {{ availableGroup.name.charAt(0).toUpperCase() }}
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-medium text-primary truncate">{{ availableGroup.name }}</span>
                      <span
                        v-if="availableGroup.external_source"
                        class="px-1.5 py-0.5 text-xs bg-accent/10 text-accent rounded-full flex items-center gap-1"
                      >
                        <Icon name="refresh" size="xs" />
                        Synced
                      </span>
                    </div>
                    <div class="text-xs text-tertiary">{{ availableGroup.member_count }} member{{ availableGroup.member_count !== 1 ? 's' : '' }}</div>
                  </div>
                </div>
                <div v-if="filteredAvailableGroups.length === 0" class="p-4 text-center text-tertiary text-sm">No groups found</div>
              </div>
              <div class="flex justify-end pt-2">
                <button
                  @click="saveIncludes"
                  :disabled="savingIncludes || !hasIncludeChanges"
                  class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50 flex items-center gap-2"
                >
                  <Spinner v-if="savingIncludes" />
                  Save Included Groups
                </button>
              </div>
            </div>
          </SectionCard>

          <!-- Included In (read-only) -->
          <SectionCard v-if="group.included_in && group.included_in.length > 0" content-padding="p-0">
            <template #title>
              <div class="flex items-center justify-between">
                <span>Included In</span>
                <span class="px-2 py-0.5 text-xs bg-surface rounded-full text-secondary font-normal">{{ group.included_in.length }}</span>
              </div>
            </template>
            <div class="divide-y divide-default max-h-48 overflow-y-auto">
              <div v-for="parentGroup in group.included_in" :key="parentGroup.id" class="flex items-center gap-3 px-4 py-2.5">
                <div
                  class="w-6 h-6 rounded flex items-center justify-center text-white text-xs font-semibold flex-shrink-0"
                  :style="{ backgroundColor: parentGroup.color || '#6366f1', ...colorFilterStyle }"
                >
                  {{ parentGroup.name.charAt(0).toUpperCase() }}
                </div>
                <span class="text-sm text-primary truncate flex-1">{{ parentGroup.name }}</span>
                <span class="text-xs text-tertiary">{{ parentGroup.member_count }} member{{ parentGroup.member_count !== 1 ? 's' : '' }}</span>
              </div>
            </div>
          </SectionCard>
        </template>
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
            <Spinner v-if="isDeleting" />
            Delete Group
          </button>
        </div>
      </div>
    </Modal>

    <ConfirmModal
      :show="showUnmanageConfirm"
      variant="warning"
      :title="group ? `Unmanage ${group.name}?` : 'Unmanage group?'"
      message="The group will no longer sync with Microsoft Entra ID. Manual edits become allowed, but existing sync history is preserved."
      confirm-label="Unmanage"
      @confirm="doUnmanageGroup"
      @close="showUnmanageConfirm = false"
    />
  </div>
</template>
