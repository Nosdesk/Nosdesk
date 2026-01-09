<script setup lang="ts">
import { formatDate as formatDateUtil, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import UserCard from '@/components/UserCard.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import DeviceGroups from '@/components/DeviceGroups.vue';
import Modal from '@/components/Modal.vue';
import { getDeviceById, updateDevice, createDevice, deleteDevice, unmanageDevice } from '@/services/deviceService';
import { IntuneIcon, EntraIcon } from '@/components/icons';
import type { Device, DeviceFormData } from '@/types/device';

const route = useRoute();
const router = useRouter();
const emit = defineEmits(['update:device']);
const device = ref<Device | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const showUserSelectionModal = ref(false);
const showAdditionalDetails = ref(false);
const showUnmanageModal = ref(false);
const unmanageError = ref<string | null>(null);

// Creation and editing state
const isCreationMode = ref(false);
const isNewDevice = ref(false);
const editingName = ref(false);
const editingManufacturer = ref(false);
const editingModel = ref(false);
const editingHostname = ref(false);
const editingSerialNumber = ref(false);
const editingWarrantyStatus = ref(false);
const isSaving = ref(false);

// Editing values
const editValues = ref({
  name: '',
  manufacturer: '',
  model: '',
  hostname: '',
  serial_number: '',
  warranty_status: ''
});

const fetchDeviceData = async () => {
  try {
    loading.value = true;
    error.value = null;
    
    // Check for creation mode (no ID parameter)
    if (!route.params.id || route.params.id === 'new') {
      isCreationMode.value = true;
      isNewDevice.value = true;

      // In creation mode, emit null for device
      emit('update:device', null);

      // Set default values for new device
      editValues.value = {
        name: '',
        manufacturer: '',
        model: '',
        hostname: '',
        serial_number: '',
        warranty_status: 'Unknown'
      };

      // Enable editing mode for all fields
      editingName.value = true;
      editingManufacturer.value = true;
      editingModel.value = true;
      editingHostname.value = true;
      editingSerialNumber.value = true;
      editingWarrantyStatus.value = true;

      // Focus on hostname field after DOM update
      setTimeout(() => {
        const hostnameInput = document.getElementById('hostname-input') as HTMLInputElement;
        if (hostnameInput) {
          hostnameInput.focus();
        }
      }, 100);

      loading.value = false;
      return;
    }
    
    const deviceId = Number(route.params.id);
    if (isNaN(deviceId)) {
      error.value = 'Invalid device ID';
      loading.value = false;
      return;
    }
    
    device.value = await getDeviceById(deviceId);
    console.log('Device data loaded:', device.value);
    
    // Check if this is a new device (name starts with "New Device")
    isNewDevice.value = device.value.name.startsWith('New Device');
    
    // Set edit values
    editValues.value = {
      name: device.value.name,
      manufacturer: device.value.manufacturer || '',
      model: device.value.model,
      hostname: device.value.hostname,
      serial_number: device.value.serial_number,
      warranty_status: device.value.warranty_status
    };
    
    // If it's a new device, enable editing mode for key fields
    if (isNewDevice.value) {
      editingHostname.value = true;
      // Focus on hostname field after DOM update
      setTimeout(() => {
        const hostnameInput = document.getElementById('hostname-input') as HTMLInputElement;
        if (hostnameInput) {
          hostnameInput.focus();
          hostnameInput.select();
        }
      }, 100);
    }
  } catch (e) {
    error.value = 'Failed to load device details';
    console.error('Error loading device:', e);
  } finally {
    loading.value = false;
  }
};

// Check if the device was accessed from a ticket
const fromTicket = computed(() => {
  return route.query.fromTicket ? Number(route.query.fromTicket) : null;
});

// Navigate back to the ticket if needed
const navigateToTicket = () => {
  if (fromTicket.value) {
    router.push(`/tickets/${fromTicket.value}`);
  }
};

const formatDate = (dateString: string) => {
  try {
    const date = new Date(dateString);
    const now = new Date();
    const diffTime = now.getTime() - date.getTime();
    const diffMinutes = Math.floor(diffTime / (1000 * 60));

    if (diffMinutes < 1) {
      return 'just now';
    } else if (diffMinutes < 60) {
      return `${diffMinutes} minute${diffMinutes === 1 ? '' : 's'} ago`;
    } else if (diffMinutes < 1440) { // less than 24 hours
      const hours = Math.floor(diffMinutes / 60);
      return `${hours} hour${hours === 1 ? '' : 's'} ago`;
    } else {
      return formatDateUtil(dateString, "MMM d, yyyy");
    }
  } catch (e) {
    return dateString;
  }
};

const openInIntune = () => {
  if (device.value?.intune_device_id) {
    const url = `https://intune.microsoft.com/#view/Microsoft_Intune_Devices/DeviceSettingsMenuBlade/~/overview/mdmDeviceId/${device.value.intune_device_id}`;
    window.open(url, '_blank', 'noopener,noreferrer');
  }
};

const openInEntra = () => {
  if (device.value?.entra_device_id) {
    const url = `https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/${device.value.entra_device_id}`;
    window.open(url, '_blank', 'noopener,noreferrer');
  }
};

// Save device (create or update)
const saveDevice = async () => {
  try {
    isSaving.value = true;
    
    if (isCreationMode.value) {
      // Create new device - use hostname as the name
      const deviceData: DeviceFormData = {
        name: editValues.value.hostname || editValues.value.name,
        manufacturer: editValues.value.manufacturer,
        model: editValues.value.model,
        hostname: editValues.value.hostname,
        serial_number: editValues.value.serial_number,
        warranty_status: editValues.value.warranty_status,
        type: 'Other' // Default type, could be made editable later
      };
      
      const newDevice = await createDevice(deviceData);
      
      // Navigate to the newly created device (replace history so back button goes to devices list)
      router.replace(`/devices/${newDevice.id}`);
    } else {
      // Update existing device
      if (!device.value) return;
      
      const updatedDevice = await updateDevice(device.value.id, {
        name: editValues.value.name,
        manufacturer: editValues.value.manufacturer,
        model: editValues.value.model,
        hostname: editValues.value.hostname,
        serial_number: editValues.value.serial_number,
        warranty_status: editValues.value.warranty_status
      });
      
      // Update the device data
      device.value = { ...device.value, ...updatedDevice };
      
      // Exit edit mode for all fields
      editingName.value = false;
      editingManufacturer.value = false;
      editingModel.value = false;
      editingHostname.value = false;
      editingSerialNumber.value = false;
      editingWarrantyStatus.value = false;
      isNewDevice.value = false;
    }
  } catch (err) {
    console.error('Error saving device:', err);
    error.value = 'Failed to save device. Please try again.';
  } finally {
    isSaving.value = false;
  }
};

// Handle name updates from InlineEdit component
const handleNameUpdate = (newName: string) => {
  if (newName !== (device.value?.name || '') && newName.trim() !== '') {
    saveField('name');
  }
};

// Simple save function for individual fields (for existing devices)
const saveField = async (field: keyof typeof editValues.value) => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      [field]: editValues.value[field]
    });

    // Update the device data
    device.value = { ...device.value, ...updatedDevice };
    // Note: Title update is handled automatically by the hostname watcher

    // Exit edit mode for this field
    switch (field) {
      case 'name':
        editingName.value = false;
        isNewDevice.value = false; // No longer a new device after saving name
        break;
      case 'manufacturer':
        editingManufacturer.value = false;
        break;
      case 'model':
        editingModel.value = false;
        break;
      case 'hostname':
        editingHostname.value = false;
        break;
      case 'serial_number':
        editingSerialNumber.value = false;
        break;
      case 'warranty_status':
        editingWarrantyStatus.value = false;
        break;
    }
  } catch (error) {
    console.error('Error saving device field:', error);
    // Reset the edit value to original
    if (device.value) {
      editValues.value[field] = device.value[field] || '';
    }
  } finally {
    isSaving.value = false;
  }
};

// Cancel editing
const cancelEdit = (field: keyof typeof editValues.value) => {
  if (!device.value) return;
  
  // Reset to original value
  editValues.value[field] = device.value[field] || '';
  
  // Exit edit mode
  switch (field) {
    case 'name':
      editingName.value = false;
      break;
    case 'manufacturer':
      editingManufacturer.value = false;
      break;
    case 'model':
      editingModel.value = false;
      break;
    case 'hostname':
      editingHostname.value = false;
      break;
    case 'serial_number':
      editingSerialNumber.value = false;
      break;
    case 'warranty_status':
      editingWarrantyStatus.value = false;
      break;
  }
};

// Handle user selection from modal
const handleUserSelection = async (user: { uuid: string; name: string; email: string; role: string }) => {
  if (!device.value) return;

  try {
    isSaving.value = true;
    const updatedDevice = await updateDevice(device.value.id, {
      primary_user_uuid: user.uuid || null
    });

    // Update the device data with the new user information
    device.value = { ...device.value, ...updatedDevice };
  } catch (error) {
    console.error('Error updating device user:', error);
    // You could add user-facing error handling here
  } finally {
    isSaving.value = false;
  }
};

// Handle delete device (called after DeleteButton confirmation)
const handleDeleteDevice = async () => {
  if (!device.value) return;

  try {
    await deleteDevice(device.value.id);
    // Navigate back to devices list after successful deletion
    router.push('/devices');
  } catch (error) {
    console.error('Error deleting device:', error);
    alert('Failed to delete device. Please try again.');
  }
};

// Handle unmanage device - show confirmation modal
const handleUnmanageDevice = () => {
  if (!device.value) return;
  unmanageError.value = null;
  showUnmanageModal.value = true;
};

// Confirm and execute unmanage
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

// Watch device and emit device object updates
watch(device, (newDevice) => {
  // Only emit when valid device data exists - prevents title flash during loading
  // Clearing is handled by App.vue on route leave
  if (newDevice) {
    emit('update:device', newDevice);
  }
}, { immediate: true, deep: true }); // deep: true to watch nested property changes

onMounted(() => {
  fetchDeviceData();
});
</script>

<template>
  <div class="flex-1">
    <div v-if="device || isCreationMode" class="flex flex-col">
      <!-- Navigation and actions bar -->
      <div v-if="!isCreationMode" class="pt-4 px-6 flex justify-between items-center">
        <div class="flex items-center gap-4">
          <BackButton
            v-if="fromTicket"
            :fallbackRoute="`/tickets/${fromTicket}`"
            :label="`Back to Ticket #${fromTicket}`"
          />
          <BackButton
            v-else
            fallbackRoute="/devices"
            label="Go back"
          />

          <!-- Read-only indicator for Microsoft Graph synced devices -->
          <div v-if="device && !device.is_editable" class="flex items-center gap-2 text-sm">
            <div class="w-2 h-2 rounded-full bg-accent"></div>
            <span class="text-secondary">Read-only</span>
          </div>
        </div>

        <DeleteButton
          v-if="device?.is_editable"
          fallbackRoute="/devices"
          itemName="Device"
          @delete="handleDeleteDevice"
        />
      </div>

      <!-- Creation Mode Header -->
      <div v-else class="pt-4 px-6 flex items-center">
        <BackButton fallbackRoute="/devices" label="Go back" />
      </div>

      <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">

        <!-- Error Display -->
        <div v-if="error" class="bg-status-error/30 border border-status-error rounded-lg p-4 text-status-error text-sm">
          {{ error }}
        </div>

        <!-- Device Creation Form -->
        <div v-if="isCreationMode" class="grid grid-cols-1 md:grid-cols-2 gap-6">
          <!-- Basic Information -->
          <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
            <!-- Header -->
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <h2 class="text-lg font-medium text-primary">Basic Information</h2>
            </div>
            
            <!-- Content -->
            <div class="p-4">
              <div class="flex flex-col gap-4">
                <!-- Device Name -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Device Name *</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <input
                      v-model="editValues.name"
                      type="text"
                      placeholder="Enter device name"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    />
                  </div>
                </div>

                <!-- Manufacturer -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Manufacturer</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <input
                      v-model="editValues.manufacturer"
                      type="text"
                      placeholder="e.g., Dell, HP, Apple"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    />
                  </div>
                </div>

                <!-- Model -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Model</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <input
                      v-model="editValues.model"
                      type="text"
                      placeholder="Enter device model"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    />
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- System Details -->
          <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
            <!-- Header -->
            <div class="px-4 py-3 bg-surface-alt border-b border-default">
              <h2 class="text-lg font-medium text-primary">System Details</h2>
            </div>
            
            <!-- Content -->
            <div class="p-4">
              <div class="flex flex-col gap-4">
                <!-- Hostname -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Hostname</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <input
                      v-model="editValues.hostname"
                      type="text"
                      placeholder="Enter hostname (optional)"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    />
                  </div>
                </div>

                <!-- Serial Number -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Serial Number</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <input
                      v-model="editValues.serial_number"
                      type="text"
                      placeholder="Enter serial number (optional)"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    />
                  </div>
                </div>

                <!-- Warranty Status -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Status</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <select
                      v-model="editValues.warranty_status"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50"
                    >
                      <option value="Active" class="bg-surface-alt">Active</option>
                      <option value="Expired" class="bg-surface-alt">Expired</option>
                      <option value="Unknown" class="bg-surface-alt">Unknown</option>
                    </select>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Existing Device Details -->
        <div v-else-if="device" class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6 items-start">
          <!-- Left Column Wrapper (System Info + Primary User in 2-col layout) -->
          <div class="flex flex-col gap-6">
            <!-- System Information -->
            <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
              <!-- Header -->
              <div class="px-4 py-3 bg-surface-alt rounded-t-xl border-b border-default">
                <h2 class="text-lg font-medium text-primary">System Information</h2>
              </div>
              
              <!-- Content -->
              <div class="p-4">
                <div class="flex flex-col gap-4">
                  <!-- Hostname -->
                  <div class="flex flex-col gap-1.5">
                    <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Hostname</h3>
                    <InlineEdit
                      v-model="editValues.hostname"
                      :placeholder="device.hostname || 'Enter hostname...'"
                      text-size="sm"
                      :can-edit="device.is_editable"
                      @update:modelValue="() => saveField('hostname')"
                    />
                  </div>

                  <!-- Serial Number -->
                  <div class="flex flex-col gap-1.5">
                    <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Serial Number</h3>
                    <InlineEdit
                      v-model="editValues.serial_number"
                      :placeholder="device.serial_number || 'Enter serial number...'"
                      text-size="sm"
                      :can-edit="device.is_editable"
                      :monospace="true"
                      @update:modelValue="() => saveField('serial_number')"
                    />
                  </div>

                  <!-- Hardware Info -->
                  <div class="pt-2 border-t border-default">
                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div class="flex flex-col gap-1.5">
                        <span class="text-xs text-secondary uppercase tracking-wide">Manufacturer</span>
                        <InlineEdit
                          v-model="editValues.manufacturer"
                          :placeholder="device.manufacturer || 'Enter manufacturer...'"
                          text-size="sm"
                          :can-edit="device.is_editable"
                          @update:modelValue="() => saveField('manufacturer')"
                        />
                      </div>

                      <div class="flex flex-col gap-1.5">
                        <span class="text-xs text-secondary uppercase tracking-wide">Model</span>
                        <InlineEdit
                          v-model="editValues.model"
                          :placeholder="device.model || 'Enter model...'"
                          text-size="sm"
                          :can-edit="device.is_editable"
                          @update:modelValue="() => saveField('model')"
                        />
                      </div>
                    </div>
                  </div>

                  <!-- Warranty Status -->
                  <div class="pt-2 border-t border-default">
                    <div class="flex flex-col gap-2">
                      <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Status</h3>

                      <!-- Editable warranty status -->
                      <div v-if="device.is_editable" class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                        <select
                          v-model="editValues.warranty_status"
                          @change="() => saveField('warranty_status')"
                          class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary focus:outline-none focus:ring-2 focus:ring-accent/50 text-sm"
                        >
                          <option value="Active" class="bg-surface-alt">Active</option>
                          <option value="Expired" class="bg-surface-alt">Expired</option>
                          <option value="Unknown" class="bg-surface-alt">Unknown</option>
                        </select>
                      </div>

                      <!-- Read-only warranty status badge -->
                      <div v-else class="inline-flex items-center px-3 py-2 rounded-lg text-sm font-medium w-fit"
                           :class="{
                             'bg-status-success/30 text-status-success border border-status-success/30': device.warranty_status === 'Active',
                             'bg-status-warning/30 text-status-warning border border-status-warning/30': device.warranty_status === 'Warning',
                             'bg-status-error/30 text-status-error border border-status-error/30': device.warranty_status === 'Expired',
                             'bg-surface-alt text-secondary border border-default': device.warranty_status === 'Unknown'
                           }">
                        {{ device.warranty_status }}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <!-- Primary User Information (shown in left column on 2-col, hidden on 3-col) -->
            <div class="xl:hidden">
              <SectionCard content-padding="p-4">
                <template #title>Primary User</template>

                <div v-if="device.primary_user" class="flex flex-col gap-4">
                  <!-- User Profile Section -->
                  <UserCard :user="device.primary_user" avatar-size="lg" />

                  <!-- Change User Button (only for editable devices) -->
                  <button
                    v-if="device.is_editable"
                    @click="showUserSelectionModal = true"
                    class="w-full mt-4 px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                    </svg>
                    Change User
                  </button>
                </div>

                <!-- No User Assigned -->
                <div v-else class="flex flex-col items-center py-8 gap-4">
                  <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                    <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"></path>
                    </svg>
                  </div>
                  <p class="text-secondary text-sm">No user assigned to this device</p>

                  <!-- Assign User Button (only for editable devices) -->
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
            </div>

            <!-- Groups (shown in left column on 2-col layout) -->
            <div class="xl:hidden">
              <DeviceGroups :groups="device.groups" />
            </div>
          </div>

          <!-- Device Management Information -->
          <div>
            <!-- Manual Device Information (for non-synced devices) -->
            <SectionCard v-if="device.is_editable" content-padding="p-4">
              <template #title>Device Information</template>

              <div class="flex flex-col gap-4">
                <!-- Device ID -->
                <div class="flex flex-col gap-2">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Device ID</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm">{{ device.id }}</span>
                  </div>
                </div>

                <!-- Timestamps -->
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Updated</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.updated_at) }}</p>
                  </div>
                </div>

                <!-- Management Type -->
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

            <!-- Microsoft Entra/Intune Information (for synced devices) -->
            <div v-else class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
              <!-- Header -->
              <div class="px-4 py-3 bg-surface-alt rounded-t-xl border-b border-default">
                <h2 class="text-lg font-medium text-primary">Microsoft Integration</h2>
              </div>

              <!-- Content -->
              <div class="p-4">

              <div class="flex flex-col gap-6">
                <!-- Timestamps -->
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1.5">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Updated</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.updated_at) }}</p>
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
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors duration-200 text-sm font-medium"
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
                      class="flex-1 min-w-[160px] flex items-center justify-center gap-2 px-4 py-2.5 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover transition-colors duration-200 text-sm font-medium border border-default"
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
                        <path d="M8 2v3" />
                        <path d="M2 8h3" />
                        <path d="M16 22v-3" />
                        <path d="M22 16h-3" />
                      </svg>
                      {{ isSaving ? 'Processing...' : 'Unmanage from Intune/Entra' }}
                    </button>
                    <p class="text-xs text-tertiary text-center">This will convert the device to manual management</p>
                  </div>

                  <!-- Additional Details Dropdown -->
                  <div class="pt-4 border-t border-default">
                    <button
                      @click="showAdditionalDetails = !showAdditionalDetails"
                      class="w-full flex items-center justify-between text-secondary hover:text-primary transition-colors text-sm"
                    >
                      <span class="font-medium">{{ showAdditionalDetails ? 'Hide' : 'Show' }} Technical Details</span>
                      <svg
                        class="w-4 h-4 transition-transform duration-200"
                        :class="{ 'rotate-180': showAdditionalDetails }"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                      >
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                      </svg>
                    </button>

                    <!-- Dropdown Content -->
                    <div
                      v-show="showAdditionalDetails"
                      class="mt-4 divide-y divide-default"
                    >
                      <!-- Device ID -->
                      <div class="flex items-center justify-between py-2.5">
                        <span class="text-sm text-secondary">Device ID</span>
                        <span class="text-sm text-primary font-mono">{{ device.id }}</span>
                      </div>

                      <!-- Intune Device ID -->
                      <div v-if="device.intune_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">Intune ID</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.intune_device_id }}</span>
                      </div>

                      <!-- Entra Device ID -->
                      <div v-if="device.entra_device_id" class="flex items-start justify-between gap-4 py-2.5">
                        <span class="text-sm text-secondary flex-shrink-0">Entra ID</span>
                        <span class="text-sm text-primary font-mono text-right break-all">{{ device.entra_device_id }}</span>
                      </div>
                    </div>
                  </div>
                </div>
                
                <!-- No Management Message -->
                <div v-if="!device.intune_device_id && !device.entra_device_id" class="text-center py-8">
                  <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full mb-4">
                    <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                    </svg>
                  </div>
                  <p class="text-secondary text-sm">This device is not managed by Microsoft Intune</p>
                </div>
              </div>
              </div>
            </div>
          </div>

          <!-- Primary User Information and Groups (shown as 3rd column on xl) -->
          <div class="hidden xl:flex xl:flex-col xl:gap-6">
            <SectionCard content-padding="p-4">
              <template #title>Primary User</template>

              <div v-if="device.primary_user" class="flex flex-col gap-4">
                <!-- User Profile Section -->
                <UserCard :user="device.primary_user" avatar-size="lg" />

                <!-- Change User Button (only for editable devices) -->
                <button
                  v-if="device.is_editable"
                  @click="showUserSelectionModal = true"
                  class="w-full mt-4 px-4 py-2.5 bg-accent text-white rounded-lg hover:bg-accent/80 transition-colors text-sm font-medium flex items-center justify-center gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7h12m0 0l-4-4m4 4l-4 4m0 6H4m0 0l4 4m-4-4l4-4" />
                  </svg>
                  Change User
                </button>
              </div>

              <!-- No User Assigned -->
              <div v-else class="flex flex-col items-center py-8 gap-4">
                <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full">
                  <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"></path>
                  </svg>
                </div>
                <p class="text-secondary text-sm">No user assigned to this device</p>

                <!-- Assign User Button (only for editable devices) -->
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

            <!-- Groups -->
            <DeviceGroups :groups="device.groups" />
          </div>
        </div>
        
        <!-- Creation Mode Buttons -->
        <div v-if="isCreationMode" class="flex justify-end mt-6">
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

    <div v-else-if="loading" class="flex justify-center items-center min-h-[200px]">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-accent"></div>
    </div>

    <div v-else class="p-6 text-center text-secondary">
      Device not found
    </div>

    <!-- User Selection Modal -->
    <UserSelectionModal
      :show="showUserSelectionModal"
      :currentUserId="device?.primary_user_uuid ?? null"
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
          <!-- Broken chain icon -->
          <svg class="h-6 w-6 text-status-warning" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18.84 12.25l1.72-1.71h-.02a5.004 5.004 0 00-.12-7.07 5.006 5.006 0 00-6.95 0l-1.72 1.71" />
            <path d="M5.17 11.75l-1.71 1.71a5.004 5.004 0 00.12 7.07 5.006 5.006 0 006.95 0l1.71-1.71" />
            <path d="M8 2v3" />
            <path d="M2 8h3" />
            <path d="M16 22v-3" />
            <path d="M22 16h-3" />
          </svg>
        </div>

        <h3 class="text-xl font-medium text-primary">Unmanage from Microsoft</h3>
        <p class="text-sm text-secondary text-center max-w-sm">
          Are you sure you want to unmanage <strong class="text-primary">{{ device?.hostname || device?.name }}</strong> from Microsoft Intune/Entra?
        </p>
        <p class="text-xs text-tertiary text-center max-w-sm">
          This will convert the device to manual management. You'll be able to edit all fields, but the device will no longer sync with Microsoft.
        </p>

        <!-- Error message -->
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

<style scoped>
.transition-all {
  transition-property: all;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

.transition-colors {
  transition-property: color, background-color, border-color, text-decoration-color, fill, stroke;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
  transition-duration: 150ms;
}

.transition-opacity {
  transition-property: opacity;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
  transition-duration: 200ms;
}

@media (prefers-reduced-motion: reduce) {
  .transition-all,
  .transition-colors,
  .transition-opacity {
    transition: opacity 0.1s ease-in-out;
    transform: none;
  }
}
</style> 