<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import BackButton from '@/components/common/BackButton.vue';
import InlineEdit from '@/components/common/InlineEdit.vue';
import UserProfileCard from '@/components/settings/UserProfileCard.vue';
import { getDeviceById, updateDevice, createDevice } from '@/services/deviceService';
import MicrosoftGraphService from '@/services/MicrosoftGraphService';
import { IntuneIcon, EntraIcon } from '@/components/icons';
import type { Device, DeviceFormData } from '@/types/device';

const route = useRoute();
const router = useRouter();
const emit = defineEmits(['update:device']);
const device = ref<Device | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
const loadingObjectId = ref(false);
const entraObjectId = ref<string | null>(null);
const objectIdError = ref<string | null>(null);

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
    
    // Check if we're in creation mode (no ID parameter)
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
      return date.toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      });
    }
  } catch (e) {
    return dateString;
  }
};

const fetchEntraObjectId = async () => {
  try {
    loadingObjectId.value = true;
    objectIdError.value = null;
    
    if (!device.value || !device.value.entra_device_id) {
      objectIdError.value = 'No Entra Device ID found';
      return;
    }
    
    const response = await MicrosoftGraphService.getEntraObjectId(device.value.entra_device_id);
    if (response.success) {
      entraObjectId.value = response.object_id;
    } else {
      objectIdError.value = response.message || 'Failed to fetch Entra Object ID';
    }
  } catch (e: any) {
    objectIdError.value = e.response?.data?.message || 'Failed to fetch Entra Object ID';
    console.error('Error fetching Entra Object ID:', e);
  } finally {
    loadingObjectId.value = false;
  }
};

const openInIntune = () => {
  if (device.value?.intune_device_id) {
    const url = `https://intune.microsoft.com/#view/Microsoft_Intune_Devices/DeviceSettingsMenuBlade/~/overview/mdmDeviceId/${device.value.intune_device_id}`;
    window.open(url, '_blank', 'noopener,noreferrer');
  }
};

const openInEntra = async () => {
  if (!device.value?.entra_device_id) {
    objectIdError.value = 'No Entra Device ID found';
    return;
  }

  // If we already have the Object ID, open directly
  if (entraObjectId.value) {
    const url = `https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/${entraObjectId.value}`;
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }

  // Otherwise, fetch the Object ID first
  await fetchEntraObjectId();
  
  // If successful, open the link
  if (entraObjectId.value) {
    const url = `https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/${entraObjectId.value}`;
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

// Watch device and emit device object updates
watch(device, (newDevice) => {
  // Pass the actual reactive device object reference
  emit('update:device', newDevice);
}, { immediate: true, deep: true }); // deep: true to watch nested property changes

onMounted(() => {
  fetchDeviceData();
});
</script>

<template>
  <div class="flex-1">
    <div v-if="device || isCreationMode" class="flex flex-col">
      <!-- Navigation and actions bar -->
      <div class="pt-4 px-6 flex justify-between items-center">
        <BackButton 
          v-if="fromTicket" 
          :fallbackRoute="`/tickets/${fromTicket}`" 
          :label="`Back to Ticket #${fromTicket}`" 
        />
        <BackButton 
          v-else 
          fallbackRoute="/devices" 
          label="Back to Devices" 
        />
      </div>
      
      <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
        <!-- Device Header -->
        <div class="bg-slate-800 rounded-2xl p-6">
          <div class="flex items-start justify-between">
            <div class="flex-1">
              <!-- Device Hostname - Inline Editing -->
              <InlineEdit
                v-if="!isCreationMode"
                v-model="editValues.hostname"
                :placeholder="device?.hostname || 'Enter hostname...'"
                text-size="2xl"
                :can-edit="true"
                @update:modelValue="() => saveField('hostname')"
              />
              <div v-else class="flex items-center gap-3 group">
                <div class="flex-1 relative">
                  <input
                    id="hostname-input"
                    v-model="editValues.hostname"
                    type="text"
                    class="w-full text-2xl font-semibold text-white px-1 py-0.5 rounded-lg hover:bg-slate-700/50 focus:bg-slate-700 focus:outline-none transition-all duration-150 border-2 border-transparent focus:border-blue-500/50 bg-slate-700/50"
                    placeholder="Enter hostname..."
                  />
                </div>
              </div>
              
              <p class="text-secondary mt-1">
                {{ isCreationMode ? 'Enter device details below' : `${device?.manufacturer || 'Unknown Manufacturer'} ${device?.model}` }}
              </p>
              <p v-if="!isCreationMode" class="text-sm text-tertiary mt-2">
                Last updated {{ device?.updated_at ? formatDate(device.updated_at) : 'unknown' }}
              </p>
            </div>
            
            
          </div>
        </div>

        <!-- Error Display -->
        <div v-if="error" class="bg-status-error/30 border border-status-error rounded-lg p-4 text-status-error text-sm">
          {{ error }}
        </div>

        <!-- Device Creation Form -->
        <div v-if="isCreationMode" class="grid grid-cols-1 xl:grid-cols-2 gap-6">
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
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
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
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
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
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
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
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
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
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary placeholder-secondary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
                    />
                  </div>
                </div>

                <!-- Warranty Status -->
                <div class="flex flex-col gap-1.5">
                  <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Status</h3>
                  <div class="bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors">
                    <select
                      v-model="editValues.warranty_status"
                      class="w-full bg-transparent border-none rounded-lg px-3 py-2.5 text-primary focus:outline-none focus:ring-2 focus:ring-brand-blue/50"
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
        <div v-else-if="device" class="grid grid-cols-1 xl:grid-cols-3 gap-6">
          <!-- System Information -->
          <div class="xl:col-span-1">
            <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
              <!-- Header -->
              <div class="px-4 py-3 bg-surface-alt rounded-t-xl border-b border-default">
                <h2 class="text-lg font-medium text-primary">System Information</h2>
              </div>
              
              <!-- Content -->
              <div class="p-4">
                <div class="flex flex-col gap-4">
                  <!-- Basic Info -->
                  <div class="grid grid-cols-1 gap-3">
                    <div class="flex flex-col gap-1.5">
                      <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Hostname</h3>
                      <div class="bg-surface-alt rounded-lg border border-default p-3">
                        <span class="text-primary font-mono text-sm">{{ device.hostname }}</span>
                      </div>
                    </div>

                    <div class="flex flex-col gap-1.5">
                      <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Serial Number</h3>
                      <div class="bg-surface-alt rounded-lg border border-default p-3">
                        <span class="text-primary font-mono text-sm">{{ device.serial_number }}</span>
                      </div>
                    </div>
                  </div>

                  <!-- Hardware Info -->
                  <div class="pt-2 border-t border-default">
                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div class="flex flex-col gap-1">
                        <span class="text-xs text-secondary uppercase tracking-wide">Manufacturer</span>
                        <span class="text-primary text-sm">{{ device.manufacturer || 'Unknown' }}</span>
                      </div>

                      <div class="flex flex-col gap-1">
                        <span class="text-xs text-secondary uppercase tracking-wide">Model</span>
                        <span class="text-primary text-sm">{{ device.model }}</span>
                      </div>
                    </div>
                  </div>

                  <!-- Warranty Status -->
                  <div class="pt-2 border-t border-default">
                    <div class="flex flex-col gap-2">
                      <h3 class="text-xs font-medium text-secondary uppercase tracking-wide">Warranty Status</h3>
                      <div class="inline-flex items-center px-3 py-2 rounded-lg text-sm font-medium w-fit"
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
          </div>

          <!-- Microsoft Entra/Intune Information -->
          <div class="xl:col-span-1">
            <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
              <!-- Header -->
              <div class="px-4 py-3 bg-surface-alt rounded-t-xl border-b border-default">
                <h2 class="text-lg font-medium text-primary">Microsoft Entra/Intune</h2>
              </div>
              
              <!-- Content -->
              <div class="p-4">
              
              <!-- Device IDs Section -->
              <div class="flex flex-col gap-6">
                <!-- Intune Device ID -->
                <div v-if="device.intune_device_id" class="flex flex-col gap-2">
                  <h3 class="text-sm font-medium text-primary uppercase tracking-wide">Intune Device ID</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm break-all">{{ device.intune_device_id }}</span>
                  </div>
                </div>

                <!-- Entra Device ID -->
                <div v-if="device.entra_device_id" class="flex flex-col gap-2">
                  <h3 class="text-sm font-medium text-primary uppercase tracking-wide">Entra Device ID</h3>
                  <div class="bg-surface-alt rounded-lg p-3 border border-default">
                    <span class="text-primary font-mono text-sm break-all">{{ device.entra_device_id }}</span>
                  </div>
                </div>

                <!-- Timestamps -->
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Created</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.created_at) }}</p>
                  </div>
                  <div class="flex flex-col gap-1">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">Last Updated</h4>
                    <p class="text-primary text-sm">{{ formatDate(device.updated_at) }}</p>
                  </div>
                </div>

                <!-- Action Buttons -->
                <div class="flex flex-col gap-4 pt-4 border-t border-default">
                  <h3 class="text-sm font-medium text-primary uppercase tracking-wide mb-4">Quick Actions</h3>
                  <div class="flex flex-col sm:flex-row gap-3">
                    <button
                      v-if="device.intune_device_id"
                      @click="openInIntune"
                      class="flex items-center justify-center gap-3 px-4 py-3 bg-brand-blue text-white rounded-lg hover:bg-brand-blue/80 transition-colors duration-200 text-sm font-medium"
                    >
                      <IntuneIcon size="18" class="text-white flex-shrink-0" />
                      <span>Open in Intune</span>
                    </button>

                    <button
                      v-if="device.entra_device_id"
                      @click="openInEntra"
                      :disabled="loadingObjectId"
                      class="flex items-center justify-center gap-3 px-4 py-3 bg-surface-alt text-primary rounded-lg hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors duration-200 text-sm font-medium"
                    >
                      <EntraIcon v-if="!loadingObjectId" size="18" class="text-white flex-shrink-0" />
                      <svg v-else class="w-4 h-4 animate-spin flex-shrink-0" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                        <path class="opacity-75" fill="currentColor" d="m4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                      </svg>
                      <span>{{ loadingObjectId ? 'Loading...' : 'Open in Entra Admin Center' }}</span>
                    </button>
                  </div>
                  
                  <!-- Error Message -->
                  <div v-if="objectIdError" class="mt-3 p-3 bg-status-error/20 border border-status-error/30 rounded-lg">
                    <p class="text-status-error text-sm flex items-center gap-2">
                      <svg class="w-4 h-4 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
                        <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
                      </svg>
                      {{ objectIdError }}
                    </p>
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

          <!-- User Account Information -->
          <div class="xl:col-span-1">
            <div class="flex flex-col">
              <!-- Primary User Heading -->
              <div class="mb-4">
                <h2 class="text-lg font-medium text-primary">Primary User</h2>
              </div>

              <!-- User Profile Card (Compact Variant) -->
              <div v-if="device.primary_user">
                <UserProfileCard
                  :user="device.primary_user"
                  variant="compact"
                  :show-banner="true"
                  :show-pronouns="false"
                  :show-email="true"
                  :can-edit="false"
                  :enable-avatar-navigation="true"
                >
                  <!-- User UUID -->
                  <div class="flex flex-col gap-2">
                    <h4 class="text-xs font-medium text-secondary uppercase tracking-wide">User UUID</h4>
                    <div class="bg-surface-alt rounded-lg p-3 border border-default">
                      <span class="text-primary font-mono text-sm break-all">{{ device.primary_user.uuid }}</span>
                    </div>
                  </div>
                </UserProfileCard>
              </div>

              <!-- No User Assigned -->
              <div v-else class="bg-surface rounded-xl border border-default hover:border-strong transition-colors">
                <div class="text-center py-8 px-6">
                  <div class="inline-flex items-center justify-center w-12 h-12 bg-surface-alt rounded-full mb-4">
                    <svg class="w-6 h-6 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"></path>
                    </svg>
                  </div>
                  <p class="text-secondary text-sm">No user assigned to this device</p>
                </div>
              </div>
            </div>
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
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-brand-blue"></div>
    </div>

    <div v-else class="p-6 text-center text-secondary">
      Device not found
    </div>
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