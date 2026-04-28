<script setup lang="ts">
import { ref, computed, watch, onMounted, nextTick } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { formatDateTime } from '@/utils/dateUtils';
import BackButton from '@/components/common/BackButton.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import UserCard from '@/components/UserCard.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/DeviceGroups.vue';
import PluginSlot from '@/plugins/components/PluginSlot.vue';
import Modal from '@/components/Modal.vue';
import { getDeviceById, updateDevice, createDevice, deleteDevice, unmanageDevice } from '@/services/deviceService';
import { useSSEListeners } from '@/composables/useSSEListeners';
import type { DeviceUpdatedEventData, DeviceDeletedEventData } from '@/types/sse';
import { IntuneIcon, EntraIcon } from '@/components/icons';
import type { Device, DeviceFormData } from '@/types/device';

const route = useRoute();
const router = useRouter();
const emit = defineEmits(['update:device']);

// State
const device = ref<Device | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const isSaving = ref(false);
const showUserSelectionModal = ref(false);
const showAdditionalDetails = ref(false);
const showUnmanageModal = ref(false);
const unmanageError = ref<string | null>(null);
const hostnameRef = ref<HTMLInputElement | null>(null);
const selectedUser = ref<{ uuid: string; name: string; email: string; role: string } | null>(null);

const editValues = ref({
  name: '',
  manufacturer: '',
  model: '',
  hostname: '',
  serial_number: '',
  warranty_status: 'Unknown',
  warranty_start_date: '' as string,
  warranty_end_date: '' as string,
  purchase_date: '' as string,
  asset_tag: '' as string,
});

// Computed
const isCreationMode = computed(() => !route.params.id || route.params.id === 'new');

const fromTicket = computed(() =>
  route.query.fromTicket ? Number(route.query.fromTicket) : null
);

const isSynced = computed(() => device.value != null && !device.value.is_editable);

const warrantyOptions = [
  { value: 'Active', label: 'Active' },
  { value: 'Warning', label: 'Warning' },
  { value: 'Expired', label: 'Expired' },
  { value: 'Unknown', label: 'Unknown' }
];

// Data fetching
const fetchDeviceData = async () => {
  try {
    loading.value = true;
    error.value = null;

    if (isCreationMode.value) {
      editValues.value = {
        name: '', manufacturer: '', model: '',
        hostname: '', serial_number: '', warranty_status: 'Unknown',
        warranty_start_date: '', warranty_end_date: '',
        purchase_date: '', asset_tag: '',
      };
      emit('update:device', null);
      loading.value = false;
      await nextTick();
      hostnameRef.value?.focus();
      return;
    }

    const deviceId = Number(route.params.id);
    if (isNaN(deviceId)) {
      error.value = 'Invalid device ID';
      loading.value = false;
      return;
    }

    device.value = await getDeviceById(deviceId);
    editValues.value = {
      name: device.value.name,
      manufacturer: device.value.manufacturer || '',
      model: device.value.model,
      hostname: device.value.hostname,
      serial_number: device.value.serial_number,
      warranty_status: device.value.warranty_status,
      warranty_start_date: device.value.warranty_start_date || '',
      warranty_end_date: device.value.warranty_end_date || '',
      purchase_date: device.value.purchase_date || '',
      asset_tag: device.value.asset_tag || '',
    };
  } catch (e) {
    error.value = 'Failed to load device details';
    console.error('Error loading device:', e);
  } finally {
    loading.value = false;
  }
};

// Field saving (edit mode)
const saveField = async (field: keyof typeof editValues.value) => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      [field]: editValues.value[field]
    });
    device.value = { ...device.value, ...updatedDevice };
  } catch (err) {
    console.error('Error saving device field:', err);
    if (device.value) {
      editValues.value[field] = (device.value[field as keyof Device] as string) || '';
    }
  } finally {
    isSaving.value = false;
  }
};

// Device creation
const saveDevice = async () => {
  try {
    isSaving.value = true;
    const deviceData: DeviceFormData = {
      name: editValues.value.hostname || editValues.value.name,
      manufacturer: editValues.value.manufacturer,
      model: editValues.value.model,
      hostname: editValues.value.hostname,
      serial_number: editValues.value.serial_number,
      warranty_status: editValues.value.warranty_status,
      warranty_start_date: editValues.value.warranty_start_date || null,
      warranty_end_date: editValues.value.warranty_end_date || null,
      purchase_date: editValues.value.purchase_date || null,
      asset_tag: editValues.value.asset_tag || null,
      primary_user_uuid: selectedUser.value?.uuid || undefined,
      type: 'Other'
    };
    const newDevice = await createDevice(deviceData);
    router.replace(`/devices/${newDevice.id}`);
  } catch (err) {
    console.error('Error creating device:', err);
    error.value = 'Failed to create device. Please try again.';
  } finally {
    isSaving.value = false;
  }
};

// User selection
const handleUserSelection = async (user: { uuid: string; name: string; email: string; role: string }) => {
  if (isCreationMode.value) {
    // In create mode, just store the selection locally
    selectedUser.value = user.uuid ? user : null;
    return;
  }

  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      primary_user_uuid: user.uuid || null
    });
    device.value = { ...device.value, ...updatedDevice };
  } catch (err) {
    console.error('Error updating device user:', err);
  } finally {
    isSaving.value = false;
  }
};

// Device deletion
const handleDeleteDevice = async () => {
  if (!device.value) return;

  try {
    await deleteDevice(device.value.id);
    router.push('/devices');
  } catch (err) {
    console.error('Error deleting device:', err);
    error.value = 'Failed to delete device. Please try again.';
  }
};

// Unmanage device
const handleUnmanageDevice = () => {
  if (!device.value) return;
  unmanageError.value = null;
  showUnmanageModal.value = true;
};

const confirmUnmanageDevice = async () => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    unmanageError.value = null;
    const updatedDevice = await unmanageDevice(device.value.id);
    device.value = updatedDevice;
    showUnmanageModal.value = false;
  } catch (err) {
    console.error('Error unmanaging device:', err);
    unmanageError.value = 'Failed to unmanage device. Please try again.';
  } finally {
    isSaving.value = false;
  }
};

// External links
const openInIntune = () => {
  if (device.value?.intune_device_id) {
    window.open(
      `https://intune.microsoft.com/#view/Microsoft_Intune_Devices/DeviceSettingsMenuBlade/~/overview/mdmDeviceId/${device.value.intune_device_id}`,
      '_blank', 'noopener,noreferrer'
    );
  }
};

const openInEntra = () => {
  if (device.value?.entra_device_id) {
    window.open(
      `https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/${device.value.entra_device_id}`,
      '_blank', 'noopener,noreferrer'
    );
  }
};

// Watchers
watch(device, (newDevice) => {
  if (newDevice) {
    emit('update:device', newDevice);
  }
}, { immediate: true, deep: true });

watch(() => route.params.id, () => {
  fetchDeviceData();
});

// SSE integration for real-time updates
const { on } = useSSEListeners();

on('device-updated', (data) => {
  const event = data as DeviceUpdatedEventData;
  if (!device.value || event.device_id !== device.value.id) return;

  const field = event.field as keyof typeof editValues.value;
  if (field in editValues.value) {
    const val = typeof event.value === 'string' ? event.value : String(event.value ?? '');
    editValues.value[field] = val;
  }
  // Also update the device ref for non-editable display fields
  if (event.field in device.value) {
    (device.value as Record<string, unknown>)[event.field] = event.value;
  }
});

on('device-deleted', (data) => {
  const event = data as DeviceDeletedEventData;
  if (!device.value || event.device_id !== device.value.id) return;
  router.push('/devices');
});

// Lifecycle
onMounted(fetchDeviceData);
</script>

<template>
  <div class="flex-1">
    <!-- Loading -->
    <div v-if="loading" class="flex justify-center items-center min-h-[200px]">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <!-- Main content -->
    <div v-else-if="device || isCreationMode" class="flex flex-col">
      <!-- Navigation bar -->
      <div class="pt-4 px-6 flex justify-between items-center">
        <div class="flex items-center gap-4">
          <BackButton
            v-if="fromTicket"
            :fallbackRoute="`/tickets/${fromTicket}`"
            :label="`Back to Ticket #${fromTicket}`"
          />
          <BackButton v-else fallbackRoute="/devices" label="Go back" />

          <div v-if="isSynced" class="flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent"></div>
            <span class="text-secondary">Read-only</span>
          </div>
        </div>

        <DeleteButton
          v-if="!isCreationMode && device?.is_editable"
          fallbackRoute="/devices"
          itemName="Device"
          @delete="handleDeleteDevice"
        />
      </div>

      <!-- Content area -->
      <div class="flex flex-col gap-6 px-6 py-4 mx-auto w-full max-w-8xl">
        <AlertMessage v-if="error" type="error" :message="error" />

        <div class="grid grid-cols-1 md:grid-cols-2 gap-6 items-start">
          <!-- Left column: Device Details -->
          <SectionCard content-padding="p-4">
            <template #title>Device Details</template>

            <div class="flex flex-col gap-4">
              <!-- Name -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Name</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.name"
                  type="text"
                  placeholder="Enter device name"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.name"
                  placeholder="Enter name..."
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('name')"
                />
              </div>

              <!-- Hostname -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Hostname</h3>
                <input
                  v-if="isCreationMode"
                  ref="hostnameRef"
                  v-model="editValues.hostname"
                  type="text"
                  placeholder="Enter hostname"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.hostname"
                  :placeholder="device?.hostname || 'Enter hostname...'"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('hostname')"
                />
              </div>

              <!-- Serial Number -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Serial Number</h3>
                <input
                  v-if="isCreationMode"
                  v-model="editValues.serial_number"
                  type="text"
                  placeholder="Enter serial number"
                  class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                />
                <InlineEdit
                  v-else
                  v-model="editValues.serial_number"
                  :placeholder="device?.serial_number || 'Enter serial number...'"
                  text-size="sm"
                  :can-edit="device?.is_editable ?? false"
                  @update:modelValue="() => saveField('serial_number')"
                />
              </div>

              <!-- Manufacturer + Model side-by-side -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2 border-t border-default">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Manufacturer</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.manufacturer"
                    type="text"
                    placeholder="e.g., Dell, HP, Apple"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.manufacturer"
                    :placeholder="device?.manufacturer || 'Enter manufacturer...'"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('manufacturer')"
                  />
                </div>

                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Model</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.model"
                    type="text"
                    placeholder="Enter device model"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.model"
                    :placeholder="device?.model || 'Enter model...'"
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('model')"
                  />
                </div>
              </div>

              <!-- Warranty Status -->
              <div class="flex flex-col gap-1.5 pt-2 border-t border-default">
                <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Status</h3>
                <BaseDropdown
                  v-if="isCreationMode || device?.is_editable"
                  v-model="editValues.warranty_status"
                  :options="warrantyOptions"
                  size="sm"
                  @update:modelValue="() => { if (!isCreationMode) saveField('warranty_status') }"
                />
                <div
                  v-else
                  class="inline-flex items-center px-3 py-2 rounded-lg text-sm font-medium w-fit"
                  :class="{
                    'bg-status-success/30 text-status-success border border-status-success/30': device?.warranty_status === 'Active',
                    'bg-status-warning/30 text-status-warning border border-status-warning/30': device?.warranty_status === 'Warning',
                    'bg-status-error/30 text-status-error border border-status-error/30': device?.warranty_status === 'Expired',
                    'bg-surface-alt text-secondary border border-default': device?.warranty_status === 'Unknown'
                  }"
                >
                  {{ device?.warranty_status }}
                </div>
              </div>

              <!-- Warranty Dates -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Start</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.warranty_start_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('warranty_start_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.warranty_start_date || '-' }}</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty End</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.warranty_end_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('warranty_end_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.warranty_end_date || '-' }}</p>
                </div>
              </div>

              <!-- Purchase Date + Asset Tag -->
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Purchase Date</h3>
                  <input
                    v-if="isCreationMode || device?.is_editable"
                    v-model="editValues.purchase_date"
                    type="date"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                    @change="() => { if (!isCreationMode) saveField('purchase_date') }"
                  />
                  <p v-else class="text-primary text-sm">{{ device?.purchase_date || '-' }}</p>
                </div>
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Asset Tag</h3>
                  <input
                    v-if="isCreationMode"
                    v-model="editValues.asset_tag"
                    type="text"
                    placeholder="Enter asset tag"
                    class="w-full bg-surface-alt rounded-lg border border-default hover:border-strong px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                  />
                  <InlineEdit
                    v-else
                    v-model="editValues.asset_tag"
                    placeholder="Enter asset tag..."
                    text-size="sm"
                    :can-edit="device?.is_editable ?? false"
                    @update:modelValue="() => saveField('asset_tag')"
                  />
                </div>
              </div>
            </div>
          </SectionCard>

          <!-- Right column -->
          <div v-if="isCreationMode || device" class="flex flex-col gap-6">
            <!-- Primary User (create mode) -->
            <SectionCard v-if="isCreationMode" content-padding="p-4">
              <template #title>Primary User</template>

              <div v-if="selectedUser" class="flex flex-col gap-4">
                <UserCard :user="selectedUser" avatar-size="lg" />

                <button
                  @click="showUserSelectionModal = true"
                  class="w-full px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  Change User
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                  </svg>
                </div>
                <p class="text-secondary text-sm">No user assigned to this device</p>

                <button
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                  </svg>
                  Assign User
                </button>
              </div>
            </SectionCard>

            <!-- Primary User (edit mode) -->
            <SectionCard v-if="!isCreationMode && device" content-padding="p-4">
              <template #title>Primary User</template>

              <div v-if="device.primary_user" class="flex flex-col gap-4">
                <UserCard :user="device.primary_user" avatar-size="lg" />

                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="w-full px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  Change User
                </button>
              </div>

              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                  </svg>
                </div>
                <p class="text-secondary text-sm">No user assigned to this device</p>

                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                  </svg>
                  Assign User
                </button>
              </div>
            </SectionCard>

            <!-- Groups (edit mode only) -->
            <DeviceGroups v-if="!isCreationMode && device" :groups="device.groups" />

            <!-- Plugin panels for device info -->
            <PluginSlot v-if="!isCreationMode && device" slot-name="device-info-panels" :device="device" />

            <!-- Device Information (manual devices, edit mode only) -->
            <SectionCard v-if="!isCreationMode && device?.is_editable" content-padding="p-4">
              <template #title>Device Information</template>

              <div class="flex flex-col gap-4">
                <div class="flex flex-col gap-2">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Device ID</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm">{{ device.id }}</span>
                  </div>
                </div>

                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Updated</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>

                <div class="pt-4 border-t border-default">
                  <div class="flex items-center gap-2 text-sm">
                    <svg class="w-5 h-5 text-secondary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                    </svg>
                    <div>
                      <p class="font-medium text-primary">Manually Managed</p>
                      <p class="text-xs text-tertiary mt-0.5">This device was created and is managed manually in Nosdesk</p>
                    </div>
                  </div>
                </div>
              </div>
            </SectionCard>

            <!-- Microsoft Integration (synced devices) -->
            <SectionCard v-else-if="!isCreationMode && device" content-padding="p-4">
              <template #title>Microsoft Integration</template>

              <div class="flex flex-col gap-6">
                <!-- Timestamps -->
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Updated</h4>
                    <p class="text-primary text-sm">{{ formatDateTime(device.updated_at) }}</p>
                  </div>
                </div>

                <!-- Last Sync Time -->
                <div v-if="device.last_sync_time" class="flex flex-col gap-2 pt-4 border-t border-default">
                  <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Intune Check-in</h4>
                  <div class="flex items-center gap-2">
                    <svg class="w-4 h-4 text-accent flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                    </svg>
                    <p class="text-primary text-sm">{{ formatDateTime(device.last_sync_time) }}</p>
                  </div>
                </div>

                <!-- External Links -->
                <div class="flex flex-col gap-4 pt-4 border-t border-default">
                  <div class="flex flex-wrap gap-3">
                    <button
                      v-if="device.intune_device_id"
                      @click="openInIntune"
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium"
                    >
                      <IntuneIcon size="16" class="text-white flex-shrink-0" />
                      <span>View in Intune</span>
                      <svg class="w-3.5 h-3.5 flex-shrink-0 opacity-70" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                      </svg>
                    </button>

                    <button
                      v-if="device.entra_device_id"
                      @click="openInEntra"
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors text-sm font-medium border border-default"
                    >
                      <EntraIcon size="16" class="flex-shrink-0" />
                      <span>View in Entra</span>
                      <svg class="w-3.5 h-3.5 flex-shrink-0 opacity-70" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                      </svg>
                    </button>
                  </div>

                  <!-- Unmanage Button -->
                  <div class="pt-4 border-t border-default flex flex-col gap-3">
                    <button
                      @click="handleUnmanageDevice"
                      :disabled="isSaving"
                      class="w-full flex items-center justify-center gap-2 px-4 py-2.5 bg-status-warning/20 text-status-warning rounded-lg hover:bg-status-warning/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium"
                      title="Remove from Microsoft Intune/Entra management"
                    >
                      <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
                        <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
                        <path d="M8 2v3" /><path d="M2 8h3" /><path d="M16 22v-3" /><path d="M22 16h-3" />
                      </svg>
                      {{ isSaving ? 'Processing...' : 'Unmanage from Intune/Entra' }}
                    </button>
                    <p class="text-xs text-tertiary text-center">This will convert the device to manual management</p>
                  </div>

                  <!-- Technical Details Dropdown -->
                  <div class="pt-4 border-t border-default">
                    <button
                      @click="showAdditionalDetails = !showAdditionalDetails"
                      class="w-full flex items-center justify-between text-secondary hover:text-primary transition-colors text-sm"
                    >
                      <span class="font-medium">{{ showAdditionalDetails ? 'Hide' : 'Show' }} Technical Details</span>
                      <svg
                        class="w-4 h-4 transition-transform duration-200"
                        :class="{ 'rotate-180': showAdditionalDetails }"
                        fill="none" stroke="currentColor" viewBox="0 0 24 24"
                      >
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                      </svg>
                    </button>

                    <div v-show="showAdditionalDetails" class="mt-4 divide-y divide-default">
                      <div class="flex items-center justify-between py-2.5">
                        <span class="text-sm text-secondary">Device ID</span>
                        <span class="text-sm text-primary font-mono">{{ device.id }}</span>
                      </div>
                      <div v-if="device.intune_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">Intune ID</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.intune_device_id }}</span>
                      </div>
                      <div v-if="device.entra_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">Entra ID</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.entra_device_id }}</span>
                      </div>
                    </div>
                  </div>
                </div>

                <!-- No Management IDs fallback -->
                <div v-if="!device.intune_device_id && !device.entra_device_id" class="text-center py-8">
                  <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full mb-4">
                    <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                  </div>
                  <p class="text-secondary text-sm">This device is not managed by Microsoft Intune</p>
                </div>
              </div>
            </SectionCard>
          </div>
        </div>

        <!-- Create mode action bar -->
        <div v-if="isCreationMode" class="flex justify-end">
          <div class="flex gap-3">
            <button
              @click="router.push('/devices')"
              :disabled="isSaving"
              class="px-6 py-2.5 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover disabled:opacity-50 transition-colors text-sm font-medium"
            >
              Cancel
            </button>
            <button
              @click="saveDevice"
              :disabled="isSaving || !editValues.hostname"
              class="px-6 py-2.5 bg-status-success text-white rounded-lg hover:bg-status-success/80 disabled:opacity-50 disabled:cursor-not-allowed transition-colors text-sm font-medium flex items-center gap-2"
            >
              <svg v-if="isSaving" class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              {{ isSaving ? 'Creating...' : 'Create Device' }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Not found -->
    <div v-else class="p-6 text-center text-secondary">
      Device not found
    </div>

    <!-- User Selection Modal -->
    <UserSelectionModal
      :show="showUserSelectionModal"
      :currentUserId="isCreationMode ? (selectedUser?.uuid ?? null) : (device?.primary_user_uuid ?? null)"
      @close="showUserSelectionModal = false"
      @select-user="handleUserSelection"
    />

    <!-- Unmanage Device Confirmation Modal -->
    <Modal
      :show="showUnmanageModal"
      title="Unmanage Device"
      @close="showUnmanageModal = false"
    >
      <div class="flex flex-col items-center gap-4">
        <div class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-status-warning/20">
          <svg class="h-6 w-6 text-status-warning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
            <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
            <path d="M8 2v3" /><path d="M2 8h3" /><path d="M16 22v-3" /><path d="M22 16h-3" />
          </svg>
        </div>

        <h3 class="text-xl font-medium text-primary">Unmanage from Microsoft</h3>
        <p class="text-sm text-secondary text-center max-w-sm">
          Are you sure you want to unmanage <strong class="text-primary">{{ device?.hostname || device?.name }}</strong> from Microsoft Intune/Entra?
        </p>
        <p class="text-xs text-tertiary text-center max-w-sm">
          This will convert the device to manual management. You'll be able to edit all fields, but the device will no longer sync with Microsoft.
        </p>

        <p v-if="unmanageError" class="text-sm text-status-error text-center">
          {{ unmanageError }}
        </p>

        <div class="flex justify-center gap-3 mt-2 w-full">
          <button
            @click="showUnmanageModal = false"
            class="flex-1 px-4 py-2.5 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors border border-default"
          >
            Cancel
          </button>
          <button
            @click="confirmUnmanageDevice"
            :disabled="isSaving"
            class="flex-1 px-4 py-2.5 bg-status-warning text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Processing...' : 'Unmanage' }}
          </button>
        </div>
      </div>
    </Modal>
  </div>
</template>
